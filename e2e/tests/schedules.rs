//! End-to-end tests of `kumikit.schedules` (exit criterion E5):
//! - `command.run` "new" returns a declarative form with open workspaces and providers.
//! - `command.run` "create" saves a job and registers a kernel schedule.
//! - `schedule.run` fires the job and starts a thread whose `turn.started` carries
//!   `from: { plugin: "kumikit.schedules" }` and completes.
//! - A `reuse_thread` job sends subsequent runs into the same thread.
//! - Pausing (`schedule.setEnabled`) and deleting (`kumikit.schedules/delete`) work.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const DEADLINE: Duration = Duration::from_secs(60);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

struct Bins {
    kumikit: PathBuf,
    probe_dir: PathBuf,
    probe_wasm: PathBuf,
    schedules_wasm: PathBuf,
}

impl Bins {
    fn locate() -> Self {
        let k = std::env::var_os("KUMIKIT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root().parent().unwrap().join("kumikit"));
        let b = Bins {
            kumikit: k.join("target/debug").join(exe("kumikit")),
            probe_dir: k.join("test-plugins/probe"),
            probe_wasm: k.join("target/kumikit-test/wasm32-wasip2/release/kumikit_test_probe.wasm"),
            schedules_wasm: repo_root().join("target/wasm32-wasip2/release/kumikit_schedules.wasm"),
        };
        for f in [&b.kumikit, &b.probe_wasm, &b.schedules_wasm] {
            assert!(f.is_file(), "{} is missing", f.display());
        }
        b
    }

    fn pack(&self, dir: &Path, out: &Path) {
        let o = Command::new(&self.kumikit)
            .args(["plugin", "pack"])
            .arg(dir)
            .arg("-o")
            .arg(out)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "kumikit plugin pack {}: {}",
            dir.display(),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

struct Core {
    tmp: TempDir,
    workspace: PathBuf,
    core: Child,
    bridge: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    next_id: u64,
    events: Vec<Value>,
}

impl Core {
    fn start(bins: &Bins) -> Self {
        let tmp = tempfile::Builder::new().prefix("kksch").tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let socket = if cfg!(windows) {
            format!(
                r"\\.\pipe\kksch-{}",
                tmp.path().file_name().unwrap().to_string_lossy()
            )
        } else {
            tmp.path().join("s.sock").to_string_lossy().into_owned()
        };
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut core = Command::new(&bins.kumikit)
            .args(["serve", "--socket", &socket, "--data-dir"])
            .arg(tmp.path().join("data"))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = core.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.starts_with("ready") {
                    let _ = ready_tx.send(());
                }
            }
        });
        ready_rx.recv_timeout(DEADLINE).expect("no ready line");

        let mut bridge = Command::new(&bins.kumikit)
            .args(["bridge", "--socket", &socket])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = bridge.stdin.take().unwrap();
        let out = bridge.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        let mut c = Core {
            tmp,
            workspace,
            core,
            bridge,
            stdin,
            rx,
            next_id: 0,
            events: Vec::new(),
        };
        c.call(
            "core.hello",
            json!({ "protocol": "0.6", "client": { "name": "kumikit-schedules-e2e", "version": "0" } }),
        );
        c
    }

    fn recv(&mut self, deadline: Instant) -> Value {
        let left = deadline
            .checked_duration_since(Instant::now())
            .expect("deadline passed");
        self.rx.recv_timeout(left).expect("bridge closed")
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
        let deadline = Instant::now() + DEADLINE;
        loop {
            let m = self.recv(deadline);
            if m["method"] == "events.event" {
                self.events.push(m["params"].clone());
            } else if m["id"] == json!(id) {
                assert!(m.get("error").is_none(), "{method}: {}", m["error"]);
                return m["result"].clone();
            }
        }
    }

    fn event(&mut self, topic: &str, deadline: Instant) -> Value {
        if let Some(i) = self.events.iter().position(|e| e["topic"] == topic) {
            return self.events.remove(i);
        }
        loop {
            let m = self.recv(deadline);
            if m["method"] == "events.event" {
                if m["params"]["topic"] == topic {
                    return m["params"].clone();
                }
                self.events.push(m["params"].clone());
            }
        }
    }

    fn install_dir(&mut self, bins: &Bins, name: &str, dir: &Path) -> Value {
        let kkp = self.tmp.path().join(format!("{name}.kkp"));
        bins.pack(dir, &kkp);
        let p = self.call("plugin.install", json!({ "path": kkp }))["plugin"].clone();
        self.call(
            "plugin.enable",
            json!({ "plugin": p["id"], "grant": p["requested"] }),
        )["plugin"]
            .clone()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.bridge.kill();
        let _ = self.bridge.wait();
        let _ = self.core.kill();
        let _ = self.core.wait();
    }
}

#[test]
fn scheduled_prompts_create_run_reuse_pause_and_delete() {
    let bins = Bins::locate();
    let mut core = Core::start(&bins);

    // Install probe provider (has an `echo` model).
    let probe_stage = core.tmp.path().join("stage-probe");
    std::fs::create_dir_all(&probe_stage).unwrap();
    std::fs::copy(
        bins.probe_dir.join("plugin.toml"),
        probe_stage.join("plugin.toml"),
    )
    .unwrap();
    std::fs::copy(&bins.probe_wasm, probe_stage.join("plugin.wasm")).unwrap();
    core.install_dir(&bins, "probe", &probe_stage);

    // Install kumikit.schedules.
    let sch_src = repo_root().join("schedules");
    let sch_stage = core.tmp.path().join("stage-schedules");
    std::fs::create_dir_all(&sch_stage).unwrap();
    std::fs::copy(sch_src.join("plugin.toml"), sch_stage.join("plugin.toml")).unwrap();
    std::fs::copy(sch_src.join("README.md"), sch_stage.join("README.md")).unwrap();
    std::fs::copy(&bins.schedules_wasm, sch_stage.join("plugin.wasm")).unwrap();
    let installed = core.install_dir(&bins, "schedules", &sch_stage);
    assert_eq!(installed["id"], "kumikit.schedules");

    let ws = core.call(
        "workspace.open",
        json!({ "path": core.workspace.to_string_lossy() }),
    )["workspace"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. Command `new` returns a declarative form pre-populated with our workspace and provider.
    let form_res = core.call(
        "command.run",
        json!({ "plugin": "kumikit.schedules", "command": "new", "args": {} }),
    );
    let view = &form_res["view"];
    assert_eq!(view["model"], "form");
    assert_eq!(view["submit"], "create");
    assert_eq!(view["values"]["workspace"], ws);

    // 2. Create a `new_thread` schedule via `command.run` `create`.
    let created = core.call(
        "command.run",
        json!({
            "plugin": "kumikit.schedules",
            "command": "create",
            "args": {
                "key": "daily-check",
                "description": "Daily status check",
                "workspace": ws,
                "provider": "test.probe/probe",
                "model": "echo",
                "prompt": "hello from schedule",
                "cron": "0 9 * * *",
                "timeZone": "UTC",
                "mode": "new_thread",
                "missed": "skip"
            }
        }),
    );
    assert!(
        created["message"].as_str().unwrap().contains("daily-check"),
        "{created}"
    );

    // 3. `schedule.list` shows it with its nextRun.
    let listed = core.call("schedule.list", json!({}));
    let sched = listed["schedules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["plugin"] == "kumikit.schedules" && s["key"] == "daily-check")
        .unwrap_or_else(|| panic!("schedule missing: {listed}"));
    assert_eq!(sched["enabled"], true);
    assert!(sched["nextRun"].as_str().is_some());

    // 4. Fire it now via `schedule.run` and wait for the scheduled thread to finish.
    let ws_topic = format!("workspace:{ws}");
    core.call("events.subscribe", json!({ "topic": ws_topic, "after": 0 }));
    core.call(
        "schedule.run",
        json!({ "plugin": "kumikit.schedules", "key": "daily-check" }),
    );
    let deadline = Instant::now() + DEADLINE;
    let thread_id = loop {
        let ev = core.event(&ws_topic, deadline);
        if ev["kind"] == "thread.created"
            && let Some(tid) = ev["data"]["thread"]["id"].as_str()
        {
            break tid.to_owned();
        }
    };
    let t_topic = format!("thread:{thread_id}");
    core.call("events.subscribe", json!({ "topic": t_topic, "after": 0 }));
    let mut events = Vec::new();
    loop {
        let ev = core.event(&t_topic, deadline);
        let done = ev["kind"] == "turn.ended";
        if done {
            assert_eq!(ev["data"]["outcome"], "done", "{ev}");
        }
        events.push(ev);
        if done {
            break;
        }
    }
    let started = events
        .iter()
        .find(|e| e["kind"] == "turn.started")
        .expect("no turn.started");
    assert_eq!(started["data"]["from"]["plugin"], "kumikit.schedules");
    let text = events
        .iter()
        .find(|e| e["kind"] == "text.done")
        .expect("no text.done");
    assert_eq!(text["data"]["text"], "hello from schedule");

    // 5. Create a `reuse_thread` job via `kumikit.schedules/create` and run it twice.
    core.call(
        "kumikit.schedules/create",
        json!({
            "key": "hourly-ping",
            "workspace": ws,
            "provider": "test.probe/probe",
            "model": "echo",
            "prompt": "ping",
            "cron": "@hourly",
            "mode": "reuse_thread"
        }),
    );
    core.call(
        "schedule.run",
        json!({ "plugin": "kumikit.schedules", "key": "hourly-ping" }),
    );
    let reused_tid = loop {
        let ev = core.event(&ws_topic, Instant::now() + DEADLINE);
        if ev["kind"] == "thread.created"
            && let Some(tid) = ev["data"]["thread"]["id"].as_str()
        {
            break tid.to_owned();
        }
    };
    let reused_topic = format!("thread:{reused_tid}");
    core.call(
        "events.subscribe",
        json!({ "topic": reused_topic, "after": 0 }),
    );
    loop {
        let ev = core.event(&reused_topic, Instant::now() + DEADLINE);
        if ev["kind"] == "turn.ended" {
            break;
        }
    }
    let outcome_deadline = Instant::now() + DEADLINE;
    loop {
        assert!(
            Instant::now() < outcome_deadline,
            "schedule never recorded ran"
        );
        let list = core.call("schedule.list", json!({}));
        let row = list["schedules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == "hourly-ping")
            .unwrap();
        if row["lastOutcome"] == "ran" {
            break;
        }
        std::thread::yield_now();
    }
    core.call(
        "schedule.run",
        json!({ "plugin": "kumikit.schedules", "key": "hourly-ping" }),
    );
    loop {
        let ev = core.event(&reused_topic, Instant::now() + DEADLINE);
        if ev["kind"] == "turn.ended" {
            break;
        }
    }
    let threads = core.call("thread.list", json!({ "workspace": ws }))["threads"]
        .as_array()
        .unwrap()
        .len();
    // 1 thread from `daily-check` + 1 reused thread from two runs of `hourly-ping`.
    assert_eq!(threads, 2, "reuse_thread should not open a third thread");

    // 6. Pause and delete.
    core.call(
        "schedule.setEnabled",
        json!({ "plugin": "kumikit.schedules", "key": "daily-check", "enabled": false }),
    );
    let listed = core.call("schedule.list", json!({}));
    let paused = listed["schedules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "daily-check")
        .unwrap();
    assert_eq!(paused["enabled"], false);

    core.call("kumikit.schedules/delete", json!({ "key": "daily-check" }));
    let jobs = core.call("kumikit.schedules/list", json!({}))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["key"], "hourly-ping");
}

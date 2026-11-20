//! `kumikit.schedules`: scheduled prompts built only with the public `kumikit-plugin` SDK
//! (`kumikit:plugin/plugin@0.2.0`), using `kumikit:host/timers`, `kumikit:host/client`, and
//! `kumikit:host/kv`.

pub mod jobs;

use kumikit_plugin::client;
use kumikit_plugin::timers::{self, Missed, Schedule};
use kumikit_plugin::{CallContext, CommandResult, Error, ErrorCode, Plugin, kv};
use serde_json::{Value, json};

use crate::jobs::{Job, MissedPolicy, ThreadMode};

const JOB_PREFIX: &str = "job:";

struct Schedules;

fn err(code: ErrorCode, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn kv_key(key: &str) -> String {
    format!("{JOB_PREFIX}{key}")
}

fn load_job(key: &str) -> Result<Option<Job>, Error> {
    let Some(bytes) = kv::get(&kv_key(key))? else {
        return Ok(None);
    };
    let job: Job = serde_json::from_slice(&bytes)
        .map_err(|e| err(ErrorCode::Internal, format!("corrupt job {key}: {e}")))?;
    Ok(Some(job))
}

fn save_job(job: &Job) -> Result<(), Error> {
    let bytes = serde_json::to_vec(job).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
    kv::set(&kv_key(&job.key), &bytes)
}

fn list_jobs() -> Result<Vec<Job>, Error> {
    let mut out = Vec::new();
    for k in kv::list(JOB_PREFIX)? {
        if let Some(bytes) = kv::get(&k)?
            && let Ok(job) = serde_json::from_slice::<Job>(&bytes)
        {
            out.push(job);
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

fn create_job(input: &Value) -> Result<(Job, Option<String>), Error> {
    let job = jobs::parse_create_input(input).map_err(|e| err(ErrorCode::InvalidArgument, e))?;
    let next_run = timers::set(
        &job.key,
        &Schedule {
            cron: job.cron.clone(),
            time_zone: job.time_zone.clone(),
            description: job.description.clone(),
            missed: match job.missed {
                MissedPolicy::Skip => Missed::Skip,
                MissedPolicy::Once => Missed::Once,
            },
        },
    )?;
    save_job(&job)?;
    Ok((job, next_run))
}

fn delete_job(key: &str) -> Result<(), Error> {
    jobs::validate_key(key).map_err(|e| err(ErrorCode::InvalidArgument, e))?;
    timers::delete(key)?;
    kv::delete(&kv_key(key))?;
    Ok(())
}

impl Plugin for Schedules {
    fn activate() -> Result<(), Error> {
        Ok(())
    }

    fn deactivate() {}

    fn handle(method: String, params: String, _ctx: CallContext) -> Result<String, Error> {
        let input: Value = serde_json::from_str(&params).unwrap_or_else(|_| json!({}));
        match method.as_str() {
            "list" => {
                let jobs = list_jobs()?;
                Ok(json!({ "jobs": jobs }).to_string())
            }
            "create" => {
                let (job, next_run) = create_job(&input)?;
                Ok(json!({ "job": job, "nextRun": next_run }).to_string())
            }
            "delete" => {
                let key = input
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| err(ErrorCode::InvalidArgument, "key is required"))?;
                delete_job(key)?;
                Ok(json!({ "deleted": key }).to_string())
            }
            other => Err(err(ErrorCode::NotFound, format!("unknown method: {other}"))),
        }
    }

    fn run_command(id: String, args: String, _ctx: CallContext) -> Result<CommandResult, Error> {
        match id.as_str() {
            "new" => {
                let workspaces: Vec<(String, String)> = client::list_workspaces()?
                    .into_iter()
                    .map(|w| (w.id, w.name))
                    .collect();
                let providers: Vec<String> = client::list_providers()?
                    .into_iter()
                    .map(|p| p.id)
                    .collect();
                let view = jobs::new_form_view(&workspaces, &providers);
                Ok(CommandResult {
                    message: None,
                    view: Some(view.to_string()),
                })
            }
            "create" => {
                let input: Value = serde_json::from_str(&args).unwrap_or_else(|_| json!({}));
                let (job, next_run) = create_job(&input)?;
                let msg = match next_run {
                    Some(t) => format!("Scheduled `{}` (next run {t}).", job.key),
                    None => format!("Scheduled `{}`.", job.key),
                };
                Ok(CommandResult {
                    message: Some(msg),
                    view: None,
                })
            }
            other => Err(err(
                ErrorCode::NotFound,
                format!("unknown command: {other}"),
            )),
        }
    }

    fn fire(key: String, _due: String) -> Result<(), Error> {
        let Some(mut job) = load_job(&key)? else {
            return Err(err(ErrorCode::NotFound, format!("no saved job `{key}`")));
        };
        if job.mode == ThreadMode::ReuseThread
            && let Some(tid) = job.thread_id.as_deref()
        {
            match client::send(tid, &job.prompt) {
                Ok(_) => return Ok(()),
                Err(e) if e.code == ErrorCode::NotFound => {
                    job.thread_id = None;
                }
                Err(e) => return Err(e),
            }
        }
        let thread = client::create_thread(
            &job.workspace,
            job.provider.as_deref(),
            job.model.as_deref(),
            Some(&job.description),
        )?;
        client::send(&thread.id, &job.prompt)?;
        if job.mode == ThreadMode::ReuseThread {
            job.thread_id = Some(thread.id);
            save_job(&job)?;
        }
        Ok(())
    }
}

kumikit_plugin::export_plugin!(Schedules);

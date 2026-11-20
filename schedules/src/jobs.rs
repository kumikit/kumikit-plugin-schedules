//! Pure job schema, validation and declarative form builder for `kumikit.schedules`,
//! tested natively.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Whether each scheduled run opens a new thread or appends to a reused one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadMode {
    /// Start a fresh thread on every run.
    #[default]
    NewThread,
    /// Reuse one thread across runs (creating it on first run).
    ReuseThread,
}

/// Missed-run policy stored alongside the job.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedPolicy {
    /// Skip runs missed while the core was down.
    #[default]
    Skip,
    /// Catch up once when the core comes back.
    Once,
}

/// A saved scheduled prompt job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// Timer key (`[a-z0-9_-]{1,48}`).
    pub key: String,
    /// Human-readable description shown on the Schedules page.
    pub description: String,
    /// Target workspace id.
    pub workspace: String,
    /// Optional provider id (`<plugin>/<local>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Prompt text sent on each run.
    pub prompt: String,
    /// Five-field cron expression or macro (`@hourly`, `@daily`, …).
    pub cron: String,
    /// Optional IANA time zone name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// Thread creation mode.
    #[serde(default)]
    pub mode: ThreadMode,
    /// Missed-run policy.
    #[serde(default)]
    pub missed: MissedPolicy,
    /// Remembered thread id when `mode == ReuseThread`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// Validates and parses a create-job request JSON into a [`Job`].
pub fn parse_create_input(input: &Value) -> Result<Job, String> {
    let req_str = |k: &str| -> Result<String, String> {
        input
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("{k} is required"))
    };
    let opt_str = |k: &str| -> Option<String> {
        input
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let key = req_str("key")?;
    validate_key(&key)?;
    let workspace = req_str("workspace")?;
    let prompt = req_str("prompt")?;
    let cron = req_str("cron")?;
    let description = opt_str("description").unwrap_or_else(|| {
        let preview: String = prompt.chars().take(60).collect();
        format!("Prompt: {preview}")
    });
    let mode = match input.get("mode").and_then(Value::as_str) {
        Some("reuse_thread") => ThreadMode::ReuseThread,
        Some("new_thread") | None => ThreadMode::NewThread,
        Some(other) => return Err(format!("invalid mode `{other}`")),
    };
    let missed = match input.get("missed").and_then(Value::as_str) {
        Some("once") => MissedPolicy::Once,
        Some("skip") | None => MissedPolicy::Skip,
        Some(other) => return Err(format!("invalid missed policy `{other}`")),
    };
    Ok(Job {
        key,
        description,
        workspace,
        provider: opt_str("provider"),
        model: opt_str("model"),
        prompt,
        cron,
        time_zone: opt_str("timeZone").or_else(|| opt_str("time_zone")),
        mode,
        missed,
        thread_id: None,
    })
}

/// Validates a schedule key (`[a-zA-Z0-9_-]{1,48}`).
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 48 {
        return Err("key must be 1..=48 characters".into());
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("key may contain only ASCII letters, digits, `-` and `_`".into());
    }
    Ok(())
}

/// Builds the declarative `form` view returned by the `new` command.
pub fn new_form_view(workspaces: &[(String, String)], providers: &[String]) -> Value {
    let mut workspace_prop = json!({
        "type": "string",
        "title": "Workspace",
        "description": "Open workspace id where the prompt will run."
    });
    if !workspaces.is_empty() {
        workspace_prop["enum"] = json!(workspaces.iter().map(|(id, _)| id).collect::<Vec<_>>());
    }
    let mut provider_prop = json!({
        "type": "string",
        "title": "Provider",
        "description": "Provider id (<plugin>/<local>)."
    });
    if !providers.is_empty() {
        provider_prop["enum"] = json!(providers);
    }
    json!({
        "model": "form",
        "schema": {
            "type": "object",
            "required": ["key", "workspace", "prompt", "cron"],
            "properties": {
                "key": { "type": "string", "title": "Schedule name", "description": "Short identifier (letters, digits, - or _)." },
                "description": { "type": "string", "title": "Description" },
                "workspace": workspace_prop,
                "provider": provider_prop,
                "prompt": { "type": "string", "title": "Prompt" },
                "cron": { "type": "string", "title": "Cron expression", "description": "Five fields or @hourly, @daily, @weekly, @monthly." },
                "timeZone": { "type": "string", "title": "Time zone (optional IANA name)" },
                "mode": { "type": "string", "title": "Thread mode", "enum": ["new_thread", "reuse_thread"], "default": "new_thread" },
                "missed": { "type": "string", "title": "Missed runs", "enum": ["skip", "once"], "default": "skip" }
            }
        },
        "values": {
            "workspace": workspaces.first().map(|(id, _)| id.as_str()).unwrap_or(""),
            "provider": providers.first().map(String::as_str).unwrap_or(""),
            "cron": "@daily",
            "mode": "new_thread",
            "missed": "skip"
        },
        "submit": "create"
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_job_and_rejects_bad_keys() {
        let j = parse_create_input(&json!({
            "key": "daily-review",
            "workspace": "ws-1",
            "provider": "p/a",
            "prompt": "Summarize git status",
            "cron": "0 9 * * 1-5",
            "timeZone": "UTC",
            "mode": "reuse_thread",
            "missed": "once"
        }))
        .unwrap();
        assert_eq!(j.key, "daily-review");
        assert_eq!(j.mode, ThreadMode::ReuseThread);
        assert_eq!(j.missed, MissedPolicy::Once);
        assert_eq!(j.time_zone.as_deref(), Some("UTC"));

        assert!(validate_key("bad/key").is_err());
        assert!(validate_key("").is_err());
        assert!(parse_create_input(&json!({"key": "ok", "workspace": "w"})).is_err());
    }

    #[test]
    fn builds_declarative_form_with_choices() {
        let view = new_form_view(
            &[("ws-1".into(), "Repo".into())],
            &["kumikit.provider-claude/claude".into()],
        );
        assert_eq!(view["model"], "form");
        assert_eq!(view["submit"], "create");
        assert_eq!(view["values"]["workspace"], "ws-1");
        assert_eq!(
            view["schema"]["properties"]["provider"]["enum"][0],
            "kumikit.provider-claude/claude"
        );
    }
}

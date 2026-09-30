//! Claude Code hooks (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`).
//!
//! Wire format per <https://code.claude.com/docs/en/hooks>. A `command` hook
//! is used rather than `http`, because an unreachable `http` hook lets the
//! action proceed; the command hook can fail closed per action class.

use crate::{ADAPTER_VERSION, Enforcement, repo};
use cacogate_contracts::{
    ActionEnvelope, Agent, Bucket, CONTRACT_VERSION, ExecutionRecord, Harness, HarnessName, Tool,
    action_digest,
};
use serde_json::{Map, Value, json};

/// Translation errors.
#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    /// A required field is missing.
    #[error("hook payload is missing {0}")]
    Missing(&'static str),
    /// Canonicalisation failed.
    #[error("canon: {0}")]
    Canon(#[from] cacogate_contracts::canon::CanonError),
}

fn s<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, ClaudeError> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or(ClaudeError::Missing(key))
}

/// Hook event name.
pub fn event_name(payload: &Value) -> &str {
    payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Build an envelope from a `PreToolUse` payload.
pub fn pre_to_envelope(
    payload: &Value,
    harness_version: &str,
    now: &str,
) -> Result<ActionEnvelope, ClaudeError> {
    let session_id = s(payload, "session_id")?.to_owned();
    let tool_name = s(payload, "tool_name")?.to_owned();
    let tool_use_id = payload
        .get("tool_use_id")
        .and_then(Value::as_str)
        .map_or_else(|| format!("anon-{now}"), str::to_owned);
    let cwd = s(payload, "cwd")?.to_owned();
    let args: Map<String, Value> = payload
        .get("tool_input")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let repo = repo::probe(&cwd);
    let root = repo.as_ref().and_then(|r| r.root.clone());
    let digest = action_digest(&tool_name, &args, &cwd, root.as_deref())?;
    let permission_mode = payload.get("permission_mode").and_then(Value::as_str);
    let agent = payload
        .get("agent_type")
        .and_then(Value::as_str)
        .map(|a| Agent {
            role: Some(a.to_owned()),
            parent_id: None,
        });
    Ok(ActionEnvelope {
        contract_version: CONTRACT_VERSION.into(),
        envelope_id: format!("env_{tool_use_id}"),
        harness: Harness {
            name: HarnessName::ClaudeCode,
            version: harness_version.to_owned(),
            adapter_version: ADAPTER_VERSION.into(),
            // bypassPermissions / dontAsk sessions have nobody to answer a prompt
            interactive: permission_mode.map(|m| !matches!(m, "bypassPermissions" | "dontAsk")),
        },
        session_id,
        turn_id: payload
            .get("prompt_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tool_use_id,
        agent,
        model: None,
        tool: Tool {
            raw_name: tool_name,
            kind: None,
        },
        args,
        cwd,
        repo,
        surface_digest: None,
        ts: now.to_owned(),
        action_digest: digest,
    })
}

/// Build an execution record from a `PostToolUse` / `PostToolUseFailure` payload.
pub fn post_to_execution(payload: &Value, now: &str) -> Result<ExecutionRecord, ClaudeError> {
    let failed = event_name(payload) == "PostToolUseFailure";
    let resp = payload.get("tool_response");
    let exit_code = resp
        .and_then(|r| {
            r.get("exit_code")
                .or_else(|| r.get("exitCode"))
                .or_else(|| r.get("returnCode"))
        })
        .and_then(Value::as_i64);
    let interrupted = resp
        .and_then(|r| r.get("interrupted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_error = failed
        || exit_code.is_some_and(|c| c != 0)
        || resp
            .and_then(|r| r.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    Ok(ExecutionRecord {
        contract_version: CONTRACT_VERSION.into(),
        session_id: s(payload, "session_id")?.to_owned(),
        tool_use_id: s(payload, "tool_use_id")?.to_owned(),
        decision_id: None,
        executed: true,
        mode: None,
        exit_code,
        is_error: Some(is_error),
        bucket: if interrupted {
            Bucket::ApparatusFailure
        } else {
            Bucket::Result
        },
        duration_ms: None,
        ts: now.to_owned(),
    })
}

/// Native `PreToolUse` response. `Defer` prints nothing so Claude Code's own
/// permission flow applies unchanged: cacogate only ever tightens.
pub fn pre_response(e: Enforcement, packet: &str) -> Option<Value> {
    let decision = match e {
        Enforcement::Defer => return None,
        Enforcement::Ask => "ask",
        Enforcement::Deny => "deny",
    };
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": packet,
        }
    }))
}

/// Hook configuration to install (command form).
pub fn hooks_config(binary: &str) -> Value {
    let hook = |event: &str| {
        json!({
            "matcher": "*",
            "hooks": [{ "type": "command", "command": format!("{binary} hook claude-code"), "timeout": 10 }],
            "_event": event,
        })
    };
    let mut pre = hook("PreToolUse");
    let mut post = hook("PostToolUse");
    let mut fail = hook("PostToolUseFailure");
    for h in [&mut pre, &mut post, &mut fail] {
        if let Some(o) = h.as_object_mut() {
            o.remove("_event");
        }
    }
    json!({ "hooks": { "PreToolUse": [pre], "PostToolUse": [post], "PostToolUseFailure": [fail] } })
}

/// Claude Code's version, from what it exports to hook processes:
/// `AI_AGENT=claude-code_2-1-283_agent`, else the versioned install path in
/// `CLAUDE_CODE_EXECPATH` (`…/claude/versions/2.1.283`). `"unknown"` otherwise.
pub fn harness_version(ai_agent: Option<&str>, execpath: Option<&str>) -> String {
    let from_agent = ai_agent.and_then(|a| {
        let v = a.strip_prefix("claude-code_")?.split('_').next()?;
        let v = v.replace('-', ".");
        is_version(&v).then_some(v)
    });
    let from_path = || {
        execpath.and_then(|p| {
            let last = p.trim_end_matches('/').rsplit('/').next()?;
            is_version(last).then(|| last.to_owned())
        })
    };
    from_agent
        .or_else(from_path)
        .unwrap_or_else(|| "unknown".into())
}

fn is_version(v: &str) -> bool {
    !v.is_empty()
        && v.split('.').count() >= 2
        && v.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The model of the latest assistant message in the tail of a Claude Code
/// transcript (JSON lines). Lines cut off by the tail window are skipped.
pub fn model_from_transcript_tail(tail: &str) -> Option<String> {
    tail.lines().rev().find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        v.pointer("/message/model")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty() && *m != "<synthetic>")
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pre(cmd: &str) -> Value {
        json!({
            "session_id": "abc", "prompt_id": "p1", "transcript_path": "/x", "cwd": "/tmp",
            "permission_mode": "default", "hook_event_name": "PreToolUse", "tool_name": "Bash",
            "tool_input": {"command": cmd, "description": "d"}, "tool_use_id": "toolu_1"
        })
    }

    #[test]
    fn test_cc001_pre_payload_to_envelope() {
        let e = pre_to_envelope(&pre("ls"), "2.1.283", "2026-09-26T00:00:00Z").unwrap();
        assert_eq!(e.session_id, "abc");
        assert_eq!(e.tool.raw_name, "Bash");
        assert_eq!(e.args["command"], json!("ls"));
        assert_eq!(e.harness.interactive, Some(true));
        assert_eq!(e.recompute_action_digest().unwrap(), e.action_digest);
    }

    #[test]
    fn test_cc002_missing_fields_are_errors_not_panics() {
        assert!(pre_to_envelope(&json!({}), "v", "t").is_err());
        assert!(pre_to_envelope(&json!("junk"), "v", "t").is_err());
    }

    #[test]
    fn test_cc003_post_failure_is_error() {
        let p = json!({"session_id": "a", "tool_use_id": "t", "hook_event_name": "PostToolUseFailure", "tool_response": {}});
        assert_eq!(post_to_execution(&p, "t").unwrap().is_error, Some(true));
        let p = json!({"session_id": "a", "tool_use_id": "t", "hook_event_name": "PostToolUse", "tool_response": {"stdout": "ok"}});
        assert_eq!(post_to_execution(&p, "t").unwrap().is_error, Some(false));
    }

    #[test]
    fn test_cc004_response_shapes() {
        assert!(pre_response(Enforcement::Defer, "x").is_none());
        let v = pre_response(Enforcement::Deny, "why").unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], json!("deny"));
        assert_eq!(
            v["hookSpecificOutput"]["hookEventName"],
            json!("PreToolUse")
        );
    }

    #[test]
    fn test_cc005_hooks_config_covers_three_events() {
        let v = hooks_config("/usr/local/bin/cacogate");
        for ev in ["PreToolUse", "PostToolUse", "PostToolUseFailure"] {
            assert_eq!(
                v["hooks"][ev][0]["hooks"][0]["type"],
                json!("command"),
                "{ev}"
            );
        }
    }

    #[test]
    fn test_cc006_harness_version_from_hook_environment() {
        assert_eq!(
            harness_version(Some("claude-code_2-1-283_agent"), None),
            "2.1.283"
        );
        assert_eq!(
            harness_version(None, Some("/home/u/.local/share/claude/versions/2.1.283")),
            "2.1.283"
        );
        assert_eq!(
            harness_version(Some("something-else"), Some("/usr/bin/claude")),
            "unknown"
        );
        assert_eq!(harness_version(None, None), "unknown");
    }

    #[test]
    fn test_cc007_model_from_transcript_tail() {
        let tail = concat!(
            "e\":\"assistant\",\"message\":{\"model\":\"cut-off\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-haiku-4-5-20251001\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"model\":\"<synthetic>\"}}\n",
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n"
        );
        assert_eq!(
            model_from_transcript_tail(tail).as_deref(),
            Some("claude-haiku-4-5-20251001")
        );
        assert_eq!(model_from_transcript_tail("{\"type\":\"user\"}"), None);
    }
}

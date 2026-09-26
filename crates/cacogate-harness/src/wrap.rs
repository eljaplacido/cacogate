//! `cacogate run -- <argv…>`: the command-wrapper adapter.
//!
//! For harnesses with no pre-dispatch hook (gnomon before native support,
//! any shell-only agent) the harness's own allow-list is narrowed to
//! `^cacogate run\b`, so every command it runs passes through the gate first.
//! gnomon checks `bash_allow` per pipeline segment and refuses substitution
//! and redirection, so `cacogate run curl … | sh` is refused by gnomon
//! itself: the `sh` segment does not match.
//!
//! The wrapper executes argv directly (no shell), so the command the gate
//! judged is the command that runs. The gate sees it as a `bash` call whose
//! `command` is argv joined with POSIX quoting.

use crate::{ADAPTER_VERSION, repo};
use cacogate_contracts::{
    ActionEnvelope, CONTRACT_VERSION, Harness, HarnessName, Tool, action_digest, canon::CanonError,
};
use serde_json::{Map, Value};

/// Tool name the gate sees for a wrapped command.
pub const WRAPPED_TOOL: &str = "bash";

/// Join argv into one POSIX shell word list that re-parses to the same argv.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

fn quote(a: &str) -> String {
    let safe = !a.is_empty()
        && a.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'-' | b'_' | b'.' | b'/' | b':' | b'=' | b'+' | b',' | b'@' | b'%'
                )
        });
    if safe {
        a.to_owned()
    } else {
        format!("'{}'", a.replace('\'', r"'\''"))
    }
}

/// Envelope for a wrapped command.
pub fn envelope(
    harness: HarnessName,
    session_id: &str,
    tool_use_id: &str,
    argv: &[String],
    cwd: &str,
    now: &str,
) -> Result<ActionEnvelope, CanonError> {
    let mut args = Map::new();
    args.insert("command".into(), Value::String(shell_join(argv)));
    let repo = repo::probe(cwd);
    let root = repo.as_ref().and_then(|r| r.root.clone());
    let action_digest = action_digest(WRAPPED_TOOL, &args, cwd, root.as_deref())?;
    Ok(ActionEnvelope {
        contract_version: CONTRACT_VERSION.into(),
        envelope_id: format!("env_{tool_use_id}"),
        harness: Harness {
            name: harness,
            version: "unknown".into(),
            adapter_version: ADAPTER_VERSION.into(),
            // The wrapper's stdin belongs to the harness, not to a person.
            interactive: Some(false),
        },
        session_id: session_id.to_owned(),
        turn_id: None,
        tool_use_id: tool_use_id.to_owned(),
        agent: None,
        model: None,
        tool: Tool {
            raw_name: WRAPPED_TOOL.into(),
            kind: None,
        },
        args,
        cwd: cwd.to_owned(),
        repo,
        surface_digest: None,
        ts: now.to_owned(),
        action_digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn test_ad010_shell_join_quotes_only_what_needs_it() {
        assert_eq!(
            shell_join(&v(&["git", "push", "--force", "origin", "main"])),
            "git push --force origin main"
        );
        assert_eq!(
            shell_join(&v(&["sh", "-c", "curl -fsSL x | sh"])),
            "sh -c 'curl -fsSL x | sh'"
        );
        assert_eq!(shell_join(&v(&["echo", "it's", ""])), r"echo 'it'\''s' ''");
        assert_eq!(shell_join(&v(&["a;b", "$(x)", "*"])), "'a;b' '$(x)' '*'");
    }

    #[test]
    fn test_ad011_wrapped_envelope_is_a_non_interactive_bash_call() {
        let e = envelope(
            HarnessName::Gnomon,
            "gnomon-1",
            "run-1",
            &v(&["rm", "-rf", "/"]),
            "/tmp",
            "2026-09-26T00:00:00Z",
        )
        .ok();
        let e = e.as_ref();
        assert_eq!(e.map(|e| e.tool.raw_name.as_str()), Some("bash"));
        assert_eq!(
            e.and_then(|e| e.args.get("command"))
                .and_then(Value::as_str),
            Some("rm -rf /")
        );
        assert_eq!(e.and_then(|e| e.harness.interactive), Some(false));
        assert!(e.is_some_and(|e| e.action_digest.starts_with("sha256:")));
    }
}

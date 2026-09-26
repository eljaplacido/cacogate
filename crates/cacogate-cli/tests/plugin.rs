//! The Claude Code plugin launcher must not fail open when the binary is
//! missing and enforce mode is configured (Claude Code proceeds on any hook
//! exit code except 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

fn launch(mode: Option<&str>, config: Option<&str>) -> Option<i32> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters/claude-code/hooks/cacogate-hook.sh");
    let home = tempfile::tempdir().unwrap();
    let cfg = home.path().join("config.toml");
    if let Some(c) = config {
        std::fs::write(&cfg, c).unwrap();
    }
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(script)
        .env_clear()
        .env("PATH", "/nonexistent")
        .env("HOME", home.path())
        .env("CACOGATE_CONFIG", &cfg);
    if let Some(m) = mode {
        cmd.env("CACOGATE_MODE", m);
    }
    cmd.output().unwrap().status.code()
}

#[test]
fn test_ad016_plugin_launcher_fails_closed_without_binary_in_enforce_mode() {
    assert_eq!(launch(Some("enforce"), None), Some(2));
    assert_eq!(launch(None, Some("mode = \"enforce\"\n")), Some(2));
    assert_eq!(launch(None, Some("  mode = 'enforce'\n")), Some(2));
    // A mode the binary would reject is a config error there: block here too.
    assert_eq!(launch(Some("Enforce"), None), Some(2));
    // A commented-out line is not configuration.
    assert_eq!(launch(None, Some("# mode = \"enforce\"\n")), Some(0));
    // Shadow (explicit or default) only records, so there is nothing to enforce.
    assert_eq!(
        launch(Some("shadow"), Some("mode = \"enforce\"\n")),
        Some(0)
    );
    assert_eq!(launch(None, None), Some(0));
}

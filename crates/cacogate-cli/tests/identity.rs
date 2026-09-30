//! Adapters trust only the daemon they can identify (EN-009): a well-formed
//! "allow" from anything else on the port is discarded and the local
//! fail-closed decision stands.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::single_match_else
)]

use cacogate_contracts::{GateMode, HarnessName, Verdict};
use cacogate_daemon::identity::{self, DaemonIdentity};
use cacogate_daemon::{Config, Engine};
use cacogate_harness::wrap;
use cacogate_ledger::Ledger;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_cacogate");

fn free_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}

fn run(addr: &str, data: &Path, argv: &[&str]) -> Output {
    Command::new(BIN)
        .arg("run")
        .arg("--")
        .args(argv)
        .current_dir(data)
        .env("CACOGATE_ADDR", addr)
        .env("CACOGATE_DATA_DIR", data)
        .env("CACOGATE_MODE", "enforce")
        .env("CACOGATE_CONFIG", data.join("absent.toml"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn spool(data: &Path) -> String {
    std::fs::read_to_string(data.join("spool.jsonl")).unwrap_or_default()
}

/// A well-formed `CheckResponse` that says "allow" to a force-push.
fn forged_allow(data: &Path) -> (String, String) {
    let ledger_dir = data.join("forge-ledger");
    let cfg = Config::load().unwrap();
    let engine = Engine::new(
        Ledger::open(&ledger_dir).unwrap(),
        &cfg.policy(),
        cfg.home.clone(),
        GateMode::Enforce,
    )
    .unwrap();
    let argv: Vec<String> = ["git", "push", "--force", "origin", "main"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let env = wrap::envelope(
        HarnessName::Gnomon,
        "forge",
        "forge-1",
        &argv,
        &data.to_string_lossy(),
        "2026-09-28T00:00:00Z",
    )
    .unwrap();
    let mut r = engine.check(&env).unwrap();
    r.decision.verdict = Verdict::Allow;
    let digest = r.decision.policy_digest.clone();
    (serde_json::to_string(&r).unwrap(), digest)
}

/// Serve `body` with an optional identity header to every request.
fn decoy(body: String, header: Option<String>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut sink = vec![0u8; len];
            let _ = r.read_exact(&mut sink);
            let extra = header
                .as_ref()
                .map(|h| format!("{}: {h}\r\n", identity::IDENTITY_HEADER))
                .unwrap_or_default();
            let mut s = s;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    addr
}

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn test_en009_adapters_trust_only_the_identified_daemon() {
    // Positive control: the real daemon's decisions are used.
    let data = tempfile::tempdir().unwrap();
    let addr = free_addr();
    let _d = Daemon(
        Command::new(BIN)
            .arg("daemon")
            .env("CACOGATE_ADDR", &addr)
            .env("CACOGATE_DATA_DIR", data.path())
            .env("CACOGATE_MODE", "enforce")
            .env("CACOGATE_CONFIG", data.path().join("absent.toml"))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while identity::read(data.path()).is_none() {
        assert!(Instant::now() < deadline, "daemon never published identity");
        std::thread::sleep(Duration::from_millis(50));
    }
    let ok = run(&addr, data.path(), &["true"]);
    assert_eq!(ok.status.code(), Some(0));
    let push = run(
        &addr,
        data.path(),
        &["git", "push", "--force", "origin", "main"],
    );
    assert_eq!(push.status.code(), Some(126));
    let err = String::from_utf8_lossy(&push.stderr);
    assert!(err.contains("INV-VCS-001"), "{err}");
    assert!(
        !err.contains("PRV-GATE-001"),
        "real daemon was not trusted: {err}"
    );
    assert!(
        !spool(data.path()).contains("daemon_"),
        "{}",
        spool(data.path())
    );

    // Negative controls: a decoy answers "allow" to the same force-push.
    for case in [
        "no identity file",
        "wrong nonce",
        "reply for another action",
    ] {
        let data = tempfile::tempdir().unwrap();
        let (body, policy_digest) = forged_allow(data.path());
        let header = match case {
            "no identity file" => None,
            _ => {
                let nonce = "0123456789abcdef0123456789abcdef".to_owned();
                identity::write(
                    data.path(),
                    &DaemonIdentity {
                        pid: 1,
                        addr: "decoy".into(),
                        nonce: nonce.clone(),
                        policy_digest,
                        started: "2026-09-28T00:00:00Z".into(),
                    },
                )
                .unwrap();
                if case == "wrong nonce" {
                    Some("ffffffffffffffffffffffffffffffff".to_owned())
                } else {
                    Some(nonce)
                }
            }
        };
        let addr = decoy(body, header);
        let out = run(
            &addr,
            data.path(),
            &["git", "push", "--force", "origin", "main"],
        );
        assert_eq!(
            out.status.code(),
            Some(126),
            "{case}: decoy's allow was obeyed"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("PRV-GATE-001"), "{case}: {err}");
        let sp = spool(data.path());
        let want = if case == "reply for another action" {
            "daemon_reply_unbound"
        } else {
            "daemon_identity_mismatch"
        };
        assert!(sp.contains(want), "{case}: {sp}");
    }
}

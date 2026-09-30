//! Daemon identity: proof that the process answering on the gate's port is
//! the daemon that was started, not whatever else happens to hold the port.
//!
//! A liveness probe proves only that *something* answered. A stale process, a
//! foreign server on the same port, or a listener an agent started itself
//! would all pass one, and an adapter that trusted it would forward every
//! decision to a process that is not the gate. So after binding, the daemon
//! writes `<data_dir>/daemon.json` (mode 0600) with a fresh per-boot nonce and
//! its policy digest, and stamps the nonce on every reply in
//! [`IDENTITY_HEADER`]. Adapters accept a decision only when the header and the
//! decision's policy digest both match the file; otherwise they fall back to
//! the local fail-closed decision and record a degradation.
//!
//! Limit, stated plainly: a process running as the same user that reads the
//! file can echo the nonce. The file sits in the self-protected data dir and is
//! listed as a secret, so an agent reading it goes to a person (DEF-SECRET-001).
//! This defends against accidental squatters and naive decoys, not against a
//! same-user attacker with arbitrary code execution.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// File name under the data dir.
pub const IDENTITY_FILE: &str = "daemon.json";
/// Response header carrying the per-boot nonce.
pub const IDENTITY_HEADER: &str = "x-cacogate-daemon";

/// What the daemon publishes about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonIdentity {
    /// Process id.
    pub pid: u32,
    /// Address actually bound.
    pub addr: String,
    /// Per-boot random nonce (hex, 128 bits).
    pub nonce: String,
    /// Digest of the policy this daemon decides with.
    pub policy_digest: String,
    /// Start time, RFC 3339.
    pub started: String,
}

/// Why a reply was not accepted as the daemon's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityMismatch {
    /// No identity file: no daemon started with this data dir, yet something answered.
    #[error("no {IDENTITY_FILE} in the data dir, but something answered on the port")]
    NoIdentityFile,
    /// The reply carried no identity header.
    #[error("reply carried no {IDENTITY_HEADER} header")]
    NoHeader,
    /// The header does not match the file.
    #[error("reply came from a different process than the daemon that wrote {IDENTITY_FILE}")]
    WrongNonce,
    /// The decision was made under a different policy than the daemon declared.
    #[error("decision policy digest {got} does not match the daemon's {want}")]
    WrongPolicy {
        /// Digest in the decision.
        got: String,
        /// Digest the daemon declared.
        want: String,
    },
}

/// A fresh 128-bit nonce from the OS.
pub fn fresh_nonce() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().fold(String::with_capacity(32), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    }))
}

/// Path of the identity file.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(IDENTITY_FILE)
}

/// Write the identity atomically with mode 0600.
pub fn write(data_dir: &Path, id: &DaemonIdentity) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let tmp = data_dir.join(format!(".{IDENTITY_FILE}.{}", id.pid));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(&tmp)?;
    f.write_all(serde_json::to_string_pretty(id)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path(data_dir))
}

/// Read the identity, if a daemon has published one.
pub fn read(data_dir: &Path) -> Option<DaemonIdentity> {
    let t = std::fs::read_to_string(path(data_dir)).ok()?;
    serde_json::from_str(&t).ok()
}

/// Remove the file on shutdown, but only if it is still ours.
pub fn remove_if_ours(data_dir: &Path, nonce: &str) {
    if read(data_dir).is_some_and(|id| id.nonce == nonce) {
        let _ = std::fs::remove_file(path(data_dir));
    }
}

/// Decide whether a reply came from the published daemon.
pub fn verify(
    expected: Option<&DaemonIdentity>,
    header: Option<&str>,
    decision_policy_digest: Option<&str>,
) -> Result<(), IdentityMismatch> {
    let id = expected.ok_or(IdentityMismatch::NoIdentityFile)?;
    let h = header.ok_or(IdentityMismatch::NoHeader)?;
    if h != id.nonce {
        return Err(IdentityMismatch::WrongNonce);
    }
    if let Some(got) = decision_policy_digest
        && got != id.policy_digest
    {
        return Err(IdentityMismatch::WrongPolicy {
            got: got.to_owned(),
            want: id.policy_digest.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(nonce: &str) -> DaemonIdentity {
        DaemonIdentity {
            pid: 1,
            addr: "127.0.0.1:4320".into(),
            nonce: nonce.into(),
            policy_digest: "sha256:p".into(),
            started: "2026-09-28T00:00:00Z".into(),
        }
    }

    #[test]
    fn test_en007_identity_verification_rejects_every_mismatch() {
        let good = id("abc");
        assert_eq!(verify(Some(&good), Some("abc"), Some("sha256:p")), Ok(()));
        assert_eq!(
            verify(None, Some("abc"), None),
            Err(IdentityMismatch::NoIdentityFile)
        );
        assert_eq!(
            verify(Some(&good), None, None),
            Err(IdentityMismatch::NoHeader)
        );
        assert_eq!(
            verify(Some(&good), Some("abd"), None),
            Err(IdentityMismatch::WrongNonce)
        );
        assert!(matches!(
            verify(Some(&good), Some("abc"), Some("sha256:other")),
            Err(IdentityMismatch::WrongPolicy { .. })
        ));
    }

    #[test]
    fn test_en008_identity_file_is_private_and_removed_only_by_its_owner() {
        let d = tempfile::tempdir().ok();
        let Some(d) = d else { return };
        let a = id(&fresh_nonce().unwrap_or_default());
        assert_eq!(a.nonce.len(), 32);
        assert!(write(d.path(), &a).is_ok());
        assert_eq!(read(d.path()).as_ref(), Some(&a));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path(d.path()))
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0);
            assert_eq!(mode, 0o600);
        }
        remove_if_ours(d.path(), "someone-else");
        assert!(
            read(d.path()).is_some(),
            "a stranger's nonce must not remove it"
        );
        remove_if_ours(d.path(), &a.nonce);
        assert!(read(d.path()).is_none());
    }
}

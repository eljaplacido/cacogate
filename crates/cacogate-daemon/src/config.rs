//! Configuration: `~/.config/cacogate/config.toml`, overridable by env vars
//! (`CACOGATE_ADDR`, `CACOGATE_DATA_DIR`, `CACOGATE_MODE`, `CACOGATE_CONFIG`).

use cacogate_contracts::GateMode;
use serde::Deserialize;
use std::path::PathBuf;

/// Default listen address (4310 is `ZeroClaw`, 4200 `OpenFang`, 8089 `TrustLayer`).
pub const DEFAULT_ADDR: &str = "127.0.0.1:4320";

/// On-disk configuration. Unknown keys are rejected.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Listen address.
    pub addr: Option<String>,
    /// Ledger directory.
    pub data_dir: Option<String>,
    /// `shadow` or `enforce`.
    pub mode: Option<GateMode>,
    /// Test commands to request as evidence (argv lists).
    pub test_commands: Option<Vec<Vec<String>>>,
    /// Additional writable roots.
    pub extra_write_roots: Option<Vec<String>>,
    /// Additional protected branch globs.
    pub protected_branches: Option<Vec<String>>,
}

/// Resolved configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address.
    pub addr: String,
    /// Ledger directory.
    pub data_dir: PathBuf,
    /// Mode.
    pub mode: GateMode,
    /// Home directory.
    pub home: String,
    /// Raw file config (policy overrides).
    pub file: FileConfig,
    /// Where the config was read from, if a file existed.
    pub source: Option<PathBuf>,
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Unreadable file.
    #[error("read {0}: {1}")]
    Read(PathBuf, std::io::Error),
    /// Invalid TOML or unknown keys.
    #[error("parse {0}: {1}")]
    Parse(PathBuf, String),
    /// Invalid env value.
    #[error("CACOGATE_MODE must be shadow or enforce, got {0:?}")]
    Mode(String),
}

fn expand(p: &str, home: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(p)
    }
}

impl Config {
    /// Load from the default location and environment.
    pub fn load() -> Result<Config, ConfigError> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let path = std::env::var("CACOGATE_CONFIG").map_or_else(
            |_| PathBuf::from(&home).join(".config/cacogate/config.toml"),
            PathBuf::from,
        );
        let (file, source) = if path.exists() {
            let text =
                std::fs::read_to_string(&path).map_err(|e| ConfigError::Read(path.clone(), e))?;
            let f: FileConfig = toml::from_str(&text)
                .map_err(|e| ConfigError::Parse(path.clone(), e.to_string()))?;
            (f, Some(path))
        } else {
            (FileConfig::default(), None)
        };
        let addr = std::env::var("CACOGATE_ADDR")
            .ok()
            .or_else(|| file.addr.clone())
            .unwrap_or_else(|| DEFAULT_ADDR.into());
        let data_dir = std::env::var("CACOGATE_DATA_DIR")
            .ok()
            .or_else(|| file.data_dir.clone())
            .map_or_else(
                || PathBuf::from(&home).join(".local/share/cacogate"),
                |d| expand(&d, &home),
            );
        let mode = match std::env::var("CACOGATE_MODE") {
            Ok(m) if m == "shadow" => GateMode::Shadow,
            Ok(m) if m == "enforce" => GateMode::Enforce,
            Ok(m) => return Err(ConfigError::Mode(m)),
            Err(_) => file.mode.unwrap_or_default(),
        };
        Ok(Config {
            addr,
            data_dir,
            mode,
            home,
            file,
            source,
        })
    }

    /// The effective policy: builtin strict plus configured additions.
    /// Additions can only extend lists; nothing here can remove a rule.
    pub fn policy(&self) -> cacogate_kernel::Policy {
        let mut p = cacogate_kernel::Policy::builtin_strict();
        if let Some(t) = &self.file.test_commands {
            p.test_commands.clone_from(t);
        }
        if let Some(r) = &self.file.extra_write_roots {
            p.extra_write_roots.extend(
                r.iter()
                    .map(|x| expand(x, &self.home).to_string_lossy().into_owned()),
            );
        }
        if let Some(b) = &self.file.protected_branches {
            p.protected_branches.extend(b.iter().cloned());
        }
        p
    }
}

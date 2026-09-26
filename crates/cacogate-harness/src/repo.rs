//! Cheap repository probing without spawning git: walk up to `.git`, read
//! `HEAD`. Runs in the hook's hot path, so it only touches a few files.

use cacogate_contracts::Repo;
use std::path::{Path, PathBuf};

/// Find the repository containing `cwd` and read its branch and head.
pub fn probe(cwd: &str) -> Option<Repo> {
    let mut dir: Option<&Path> = Some(Path::new(cwd));
    while let Some(d) = dir {
        let dotgit = d.join(".git");
        if dotgit.exists() {
            let gitdir = resolve_gitdir(&dotgit)?;
            let head = std::fs::read_to_string(gitdir.join("HEAD")).ok();
            let (branch, commit) = match head.as_deref().map(str::trim) {
                Some(h) if h.starts_with("ref: ") => {
                    let r = h.trim_start_matches("ref: ");
                    let commit = std::fs::read_to_string(common_dir(&gitdir).join(r))
                        .ok()
                        .map(|s| s.trim().to_owned());
                    (Some(r.trim_start_matches("refs/heads/").to_owned()), commit)
                }
                Some(h) => (None, Some(h.to_owned())),
                None => (None, None),
            };
            return Some(Repo {
                root: Some(d.to_string_lossy().into_owned()),
                head: commit,
                branch,
                dirty: None,
            });
        }
        dir = d.parent();
    }
    None
}

fn resolve_gitdir(dotgit: &Path) -> Option<PathBuf> {
    if dotgit.is_dir() {
        return Some(dotgit.to_path_buf());
    }
    // Worktrees and submodules: `.git` is a file "gitdir: <path>".
    let text = std::fs::read_to_string(dotgit).ok()?;
    let p = text.trim().strip_prefix("gitdir: ")?;
    let path = PathBuf::from(p);
    Some(if path.is_absolute() {
        path
    } else {
        dotgit.parent()?.join(path)
    })
}

fn common_dir(gitdir: &Path) -> PathBuf {
    std::fs::read_to_string(gitdir.join("commondir"))
        .ok()
        .map_or_else(|| gitdir.to_path_buf(), |c| gitdir.join(c.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rp001_finds_branch() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".git/refs/heads")).unwrap();
        std::fs::write(d.path().join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        std::fs::create_dir_all(d.path().join(".git/refs/heads/feat")).unwrap();
        std::fs::write(d.path().join(".git/refs/heads/feat/x"), "abc123\n").unwrap();
        std::fs::create_dir_all(d.path().join("src/deep")).unwrap();
        let r = probe(&d.path().join("src/deep").to_string_lossy()).unwrap();
        assert_eq!(r.branch.as_deref(), Some("feat/x"));
        assert_eq!(r.head.as_deref(), Some("abc123"));
        assert_eq!(r.root.as_deref(), Some(d.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn test_rp002_no_repo() {
        let d = tempfile::tempdir().unwrap();
        // tempdirs live under /tmp, which is not normally inside a repository
        let r = probe(&d.path().to_string_lossy());
        assert!(
            r.is_none()
                || r.and_then(|r| r.root)
                    .is_some_and(|root| !root.starts_with(&*d.path().to_string_lossy()))
        );
    }
}

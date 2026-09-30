//! Cheap repository probing without spawning git: walk up to `.git`, read
//! `HEAD`. Runs in the hook's hot path, so it only touches a few files.

use cacogate_contracts::{HarnessName, Repo};
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

/// Roots that belong to the same workspace as `root`, derived from the
/// filesystem rather than taken from the adapter (article 8):
///
/// - every other checkout of the same repository (`git worktree`), found
///   through the shared common dir, so `../repo-feature` counts as the repo;
/// - for Claude Code, the session's own auto-memory directory
///   (`~/.claude/projects/<encoded path>/memory`) for `cwd` and `root`.
///   Nothing else under `~/.claude` is added; settings, hooks and plugins
///   stay under self-protection.
pub fn linked_roots(
    root: Option<&str>,
    cwd: &str,
    harness: HarnessName,
    home: &str,
) -> Vec<String> {
    let mut out: Vec<String> = root.map(worktrees).unwrap_or_default();
    if harness == HarnessName::ClaudeCode && !home.is_empty() {
        for d in std::iter::once(cwd).chain(root) {
            let m = format!(
                "{}/.claude/projects/{}/memory",
                home.trim_end_matches('/'),
                claude_project_key(d)
            );
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// Claude Code's per-project directory name: every character that is not
/// ASCII alphanumeric becomes `-` (`/home/u/a.b` → `-home-u-a-b`).
pub fn claude_project_key(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Other working trees of the repository rooted at `root`, not including it.
fn worktrees(root: &str) -> Vec<String> {
    let Some(gitdir) = resolve_gitdir(&Path::new(root).join(".git")) else {
        return Vec::new();
    };
    let common = common_dir(&gitdir);
    let common = common.canonicalize().unwrap_or(common);
    let mut out = Vec::new();
    // The main working tree owns the common dir (`<main>/.git`).
    if common.file_name().is_some_and(|n| n == ".git")
        && let Some(main) = common.parent()
    {
        out.push(main.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(common.join("worktrees")) {
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path().join("gitdir")) else {
                continue;
            };
            // `gitdir` names the linked tree's `.git` file.
            if let Some(tree) = Path::new(text.trim()).parent() {
                out.push(tree.to_path_buf());
            }
        }
    }
    let here = Path::new(root).canonicalize().ok();
    let mut roots: Vec<String> = out
        .into_iter()
        .filter(|p| p.is_dir())
        .filter(|p| p.canonicalize().ok() != here)
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    roots.sort();
    roots.dedup();
    roots
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

    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn test_rp003_linked_worktrees_are_one_workspace() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("proj");
        std::fs::create_dir_all(&main).unwrap();
        if !git(&main, &["init", "-q"])
            || !git(&main, &["commit", "-q", "--allow-empty", "-m", "i"])
        {
            return; // no git on this machine: nothing to probe
        }
        assert!(git(
            &main,
            &["worktree", "add", "-q", "../proj-feat", "-b", "feat"]
        ));
        let other = d.path().join("unrelated");
        std::fs::create_dir_all(&other).unwrap();
        assert!(git(&other, &["init", "-q"]));
        let canon = |p: &Path| p.canonicalize().unwrap().to_string_lossy().into_owned();
        let feat = d.path().join("proj-feat");

        // From the main tree the linked tree is a root, and vice versa.
        let from_main = linked_roots(Some(&canon(&main)), &canon(&main), HarnessName::Other, "");
        assert_eq!(from_main, vec![canon(&feat)]);
        let from_feat = linked_roots(Some(&canon(&feat)), &canon(&feat), HarnessName::Other, "");
        assert_eq!(from_feat, vec![canon(&main)]);
        // An unrelated repository next door is not.
        assert!(!from_main.contains(&canon(&other)));
        assert!(
            linked_roots(Some(&canon(&other)), &canon(&other), HarnessName::Other, "").is_empty()
        );
    }

    #[test]
    fn test_rp004_claude_memory_dir_is_the_sessions_own_only() {
        assert_eq!(
            claude_project_key("/home/u/Desktop/my_proj.v2"),
            "-home-u-Desktop-my-proj-v2"
        );
        let r = linked_roots(
            None,
            "/nonexistent/proj",
            HarnessName::ClaudeCode,
            "/home/u",
        );
        assert_eq!(
            r,
            vec!["/home/u/.claude/projects/-nonexistent-proj/memory".to_owned()]
        );
        // Other harnesses get no Claude Code directory.
        assert!(
            linked_roots(None, "/nonexistent/proj", HarnessName::Opencode, "/home/u").is_empty()
        );
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

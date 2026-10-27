//! Detect the `origin` remote for a worker folder.
//!
//! Prefer `.git/config` so the UI does not spawn git. Fall back to
//! `git remote get-url origin` when the config cannot be parsed (worktrees,
//! unusual layouts). Missing remotes are flagged, not errors: My Machines
//! routing uses those remotes to match a repo to this machine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemoteInfo {
    pub url: Option<String>,
    pub has_remote: bool,
}

pub fn describe_origin(dir: &str) -> GitRemoteInfo {
    let root = Path::new(dir);
    if let Some(url) = origin_from_git_dir(root) {
        return GitRemoteInfo {
            url: Some(url),
            has_remote: true,
        };
    }
    if let Some(url) = origin_from_git_cli(root) {
        return GitRemoteInfo {
            url: Some(url),
            has_remote: true,
        };
    }
    GitRemoteInfo {
        url: None,
        has_remote: false,
    }
}

fn origin_from_git_dir(root: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(root)?;
    if let Some(url) = origin_url_from_config(&git_dir.join("config")) {
        return Some(url);
    }
    // Linked worktrees store remotes on the common dir (`…/.git/config`).
    common_git_dir(&git_dir).and_then(|common| origin_url_from_config(&common.join("config")))
}

fn resolve_git_dir(root: &Path) -> Option<PathBuf> {
    let marker = root.join(".git");
    if marker.is_dir() {
        return Some(marker);
    }
    if marker.is_file() {
        let text = fs::read_to_string(&marker).ok()?;
        for line in text.lines() {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("gitdir:")?;
            let pointed = PathBuf::from(rest.trim());
            let resolved = if pointed.is_absolute() {
                pointed
            } else {
                root.join(pointed)
            };
            return resolved.canonicalize().ok().or(Some(resolved));
        }
    }
    None
}

fn common_git_dir(git_dir: &Path) -> Option<PathBuf> {
    let commondir = git_dir.join("commondir");
    if commondir.is_file() {
        let text = fs::read_to_string(commondir).ok()?;
        let pointed = PathBuf::from(text.trim());
        let resolved = if pointed.is_absolute() {
            pointed
        } else {
            git_dir.join(pointed)
        };
        return resolved.canonicalize().ok().or(Some(resolved));
    }
    git_dir.parent().and_then(|parent| {
        if git_dir.file_name()?.to_string_lossy() != "worktrees" {
            // `…/.git/worktrees/<name>` → `…/.git`
            if parent.file_name()?.to_string_lossy() == "worktrees" {
                return parent.parent().map(Path::to_path_buf);
            }
        }
        None
    })
}

fn origin_url_from_config(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    parse_origin_url(&text)
}

fn parse_origin_url(text: &str) -> Option<String> {
    let mut in_origin = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let header = line[1..line.len() - 1].trim();
            in_origin = header.eq_ignore_ascii_case("remote \"origin\"")
                || header.eq_ignore_ascii_case("remote 'origin'");
            continue;
        }
        if !in_origin {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        if key.trim().eq_ignore_ascii_case("url") {
            let url = value.trim();
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}

fn origin_from_git_cli(root: &Path) -> Option<String> {
    if !root.is_dir() {
        return None;
    }
    let mut command = Command::new("git");
    command.args(["-C", &root.to_string_lossy(), "remote", "get-url", "origin"]);
    crate::process::hide_std_window(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_origin_from_git_config() {
        let dir = tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(
            git.join("config"),
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = git@github.com:acme/app.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        )
        .unwrap();
        let info = describe_origin(&dir.path().to_string_lossy());
        assert!(info.has_remote);
        assert_eq!(info.url.as_deref(), Some("git@github.com:acme/app.git"));
    }

    #[test]
    fn flags_repo_without_origin() {
        let dir = tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("config"), "[core]\n\tbare = false\n").unwrap();
        let info = describe_origin(&dir.path().to_string_lossy());
        assert!(!info.has_remote);
        assert_eq!(info.url, None);
    }

    #[test]
    fn reads_worktree_gitdir_file() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("main");
        let work = dir.path().join("work");
        fs::create_dir_all(main.join(".git")).unwrap();
        fs::create_dir_all(&work).unwrap();
        fs::write(
            main.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/infra.git\n",
        )
        .unwrap();
        fs::write(work.join(".git"), "gitdir: ../main/.git\n").unwrap();
        let info = describe_origin(&work.to_string_lossy());
        assert_eq!(
            info.url.as_deref(),
            Some("https://github.com/acme/infra.git")
        );
    }

    #[test]
    fn parse_ignores_other_remotes() {
        let text = "[remote \"upstream\"]\nurl = git@ex:up.git\n[remote \"origin\"]\nurl = git@ex:app.git\n";
        assert_eq!(parse_origin_url(text).as_deref(), Some("git@ex:app.git"));
    }
}

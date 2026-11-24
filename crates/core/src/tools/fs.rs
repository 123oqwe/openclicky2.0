//! Workspace sandbox. Every path is resolved and verified before use (SEC-5).

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Resolve `rel` inside `root`, rejecting absolute paths, `..` escapes and
/// symlinks that point outside the workspace.
pub fn resolve_in_workspace(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Ok(root.to_path_buf());
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(Error::PolicyDenied("不允许绝对路径".into()));
    }
    for comp in candidate.components() {
        match comp {
            Component::ParentDir => return Err(Error::PolicyDenied("不允许 .. 跳出工作区".into())),
            Component::Prefix(_) | Component::RootDir => {
                return Err(Error::PolicyDenied("不允许绝对路径".into()))
            }
            _ => {}
        }
    }
    let joined = root.join(candidate);
    let root_canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    // Existing path: verify the canonical form stays inside the root.
    if joined.exists() {
        let canonical = std::fs::canonicalize(&joined).map_err(|e| Error::Io(e.to_string()))?;
        if !canonical.starts_with(&root_canonical) {
            return Err(Error::PolicyDenied("路径指向工作区之外".into()));
        }
        return Ok(canonical);
    }

    // New path: verify the closest existing ancestor.
    let mut ancestor = joined.clone();
    while !ancestor.exists() {
        if !ancestor.pop() {
            break;
        }
    }
    let ancestor_canonical = std::fs::canonicalize(&ancestor).unwrap_or_else(|_| ancestor.clone());
    if !ancestor_canonical.starts_with(&root_canonical) {
        return Err(Error::PolicyDenied("路径指向工作区之外".into()));
    }
    Ok(joined)
}

/// Does this path leave the workspace? Checked before asking for approval.
pub fn escapes_workspace(_root: &Path, rel: &str) -> bool {
    let rel = rel.trim();
    if rel.is_empty() {
        return false;
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return true;
    }
    candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

pub fn read_text(root: &Path, rel: &str, max: usize) -> Result<String> {
    let path = resolve_in_workspace(root, rel)?;
    let bytes = std::fs::read(&path).map_err(|e| Error::Io(e.to_string()))?;
    let slice = &bytes[..bytes.len().min(max)];
    Ok(String::from_utf8_lossy(slice).to_string())
}

/// Atomic write: temp file in the same directory then rename.
pub fn write_text(root: &Path, rel: &str, content: &str, append: bool) -> Result<PathBuf> {
    write_text_with(root, rel, content, append, false)
}

/// `allow_outside` is only ever true after an explicit user approval; the write
/// is still confined to paths whose parent directory exists.
pub fn write_text_with(
    root: &Path,
    rel: &str,
    content: &str,
    append: bool,
    allow_outside: bool,
) -> Result<PathBuf> {
    let path = if allow_outside && escapes_workspace(root, rel) {
        let candidate = Path::new(rel);
        let abs = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        let normalised = normalise(&abs)?;
        let parent = normalised
            .parent()
            .ok_or_else(|| Error::PolicyDenied("路径没有父目录".into()))?;
        if !parent.exists() {
            return Err(Error::PolicyDenied("批准的目标目录不存在".into()));
        }
        normalised
    } else {
        resolve_in_workspace(root, rel)?
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
    }
    if append {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::Io(e.to_string()))?;
        f.write_all(content.as_bytes())
            .map_err(|e| Error::Io(e.to_string()))?;
        return Ok(path);
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, content).map_err(|e| Error::Io(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| Error::Io(e.to_string()))?;
    Ok(path)
}

/// Delete = move into `.trash/` so a destructive action stays reversible.
/// Collapse `..` without touching the filesystem (lexical normalisation).
fn normalise(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::PolicyDenied("路径跳出了根目录".into()));
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Ok(out)
}

pub fn trash(root: &Path, rel: &str) -> Result<PathBuf> {
    let path = resolve_in_workspace(root, rel)?;
    if !path.exists() {
        return Err(Error::BadRequest(format!("文件不存在：{rel}")));
    }
    let trash_dir = root.join(".trash");
    std::fs::create_dir_all(&trash_dir).map_err(|e| Error::Io(e.to_string()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string();
    let dest = trash_dir.join(format!("{stamp}-{name}"));
    std::fs::rename(&path, &dest).map_err(|e| Error::Io(e.to_string()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_escape() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_in_workspace(dir.path(), "../../etc/passwd").is_err());
    }

    #[test]
    fn rejects_absolute() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_in_workspace(dir.path(), "/etc/passwd").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(resolve_in_workspace(dir.path(), "link/secret.txt").is_err());
    }

    #[test]
    fn writes_and_reads_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        write_text(dir.path(), "out/a.txt", "hello", false).unwrap();
        assert_eq!(read_text(dir.path(), "out/a.txt", 1000).unwrap(), "hello");
    }

    #[test]
    fn trash_keeps_the_file_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        write_text(dir.path(), "x.txt", "data", false).unwrap();
        let trashed = trash(dir.path(), "x.txt").unwrap();
        assert!(!dir.path().join("x.txt").exists());
        assert!(trashed.exists());
    }
}

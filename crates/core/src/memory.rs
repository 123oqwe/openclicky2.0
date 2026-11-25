//! Two-file memory (SRC-PRD N1–N5): a preference file that is never truncated,
//! and a project file that is compacted oldest-first.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use crate::error::{Error, Result};

pub const PROFILE_MAX_CHARS: usize = 1200;
pub const PROJECTS_MAX_CHARS: usize = 2000;

pub struct Memory {
    root: PathBuf,
    profile: Mutex<Vec<String>>,
    projects: Mutex<Vec<String>>,
    clock: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl Memory {
    pub fn load(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).map_err(|e| Error::Io(e.to_string()))?;
        let profile = read_lines(&root.join("profile.md"));
        let projects = read_lines(&root.join("projects.md"));
        Ok(Self {
            root: root.to_path_buf(),
            profile: Mutex::new(profile),
            projects: Mutex::new(projects),
            clock: Box::new(Utc::now),
        })
    }

    pub fn with_clock(mut self, f: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>) -> Self {
        self.clock = f;
        self
    }

    pub fn add_preference(&self, line: &str) -> Result<()> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        {
            let mut p = self.profile.lock().unwrap();
            if p.iter().any(|l| l == line) {
                return Ok(());
            }
            p.push(line.to_string());
        }
        self.persist_profile()
    }

    pub fn add_project_fact(&self, line: &str) -> Result<()> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        {
            let mut p = self.projects.lock().unwrap();
            if p.iter().any(|l| l == line) {
                return Ok(());
            }
            p.push(format!("- {} {}", (self.clock)().format("%Y-%m-%d"), line));
        }
        self.compact_projects()?;
        self.persist_projects()
    }

    pub fn forget(&self, needle: &str) -> Result<usize> {
        let needle = needle.trim().to_lowercase();
        // An empty needle matches every line, so a blank argument would erase
        // everything the user asked to keep.
        if needle.is_empty() {
            return Err(Error::BadRequest("要忘掉什么？给一个关键词。".into()));
        }
        let mut removed = 0;
        {
            let mut p = self.profile.lock().unwrap();
            let before = p.len();
            p.retain(|l| !l.to_lowercase().contains(&needle));
            removed += before - p.len();
        }
        {
            let mut p = self.projects.lock().unwrap();
            let before = p.len();
            p.retain(|l| !l.to_lowercase().contains(&needle));
            removed += before - p.len();
        }
        if removed > 0 {
            self.persist_profile()?;
            self.persist_projects()?;
        }
        Ok(removed)
    }

    /// Preference text is never truncated (SRC-PRD N4); over budget it is still
    /// returned in full and the caller logs a warning.
    pub fn profile_text(&self) -> String {
        self.profile.lock().unwrap().join("\n")
    }

    pub fn projects_text(&self) -> String {
        self.projects.lock().unwrap().join("\n")
    }

    pub fn profile_over_budget(&self) -> bool {
        self.profile_text().chars().count() > PROFILE_MAX_CHARS
    }

    /// Fold the oldest entries into a single `[compacted]` summary line.
    /// Terminates in one pass: keep the newest tail that fits the budget.
    pub fn compact_projects(&self) -> Result<()> {
        let mut p = self.projects.lock().unwrap();
        let summary_budget = 64;
        let lengths: Vec<usize> = p.iter().map(|l| l.chars().count() + 1).collect();
        let total: usize = lengths.iter().sum();
        if total <= PROJECTS_MAX_CHARS {
            return Ok(());
        }
        let budget = PROJECTS_MAX_CHARS.saturating_sub(summary_budget);
        let mut kept = 0usize;
        let mut used = 0usize;
        for len in lengths.iter().rev() {
            if used + len > budget || kept + 1 >= p.len() {
                break;
            }
            used += len;
            kept += 1;
        }
        let dropped = p.len().saturating_sub(kept);
        if dropped == 0 {
            return Ok(());
        }
        let summary = format!("[compacted] 更早的项目记录 {dropped} 条");
        p.drain(0..dropped);
        p.insert(0, summary);
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.profile.lock().unwrap().clear();
        self.projects.lock().unwrap().clear();
        self.persist_profile()?;
        self.persist_projects()
    }

    fn persist_profile(&self) -> Result<()> {
        let body = self.profile.lock().unwrap().join("\n");
        std::fs::write(self.root.join("profile.md"), body).map_err(|e| Error::Io(e.to_string()))
    }

    fn persist_projects(&self) -> Result<()> {
        let body = self.projects.lock().unwrap().join("\n");
        std::fs::write(self.root.join("projects.md"), body).map_err(|e| Error::Io(e.to_string()))
    }
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> (tempfile::TempDir, Memory) {
        let dir = tempfile::tempdir().unwrap();
        let m = Memory::load(dir.path()).unwrap();
        (dir, m)
    }

    #[test]
    fn preferences_are_deduplicated_and_persisted() {
        let (dir, m) = mem();
        m.add_preference("说话短一点").unwrap();
        m.add_preference("说话短一点").unwrap();
        assert_eq!(m.profile_text(), "说话短一点");
        let reloaded = Memory::load(dir.path()).unwrap();
        assert_eq!(reloaded.profile_text(), "说话短一点");
    }

    #[test]
    fn profile_is_never_truncated() {
        let (_d, m) = mem();
        for i in 0..80 {
            m.add_preference(&format!("偏好 {i} {}", "很长的内容".repeat(4)))
                .unwrap();
        }
        assert!(m.profile_over_budget());
        let text = m.profile_text();
        assert!(text.contains("偏好 79"));
        assert!(text.chars().count() > PROFILE_MAX_CHARS);
    }

    #[test]
    fn projects_are_compacted() {
        let (_d, m) = mem();
        for i in 0..200 {
            m.add_project_fact(&format!("事实 {i} {}", "细节".repeat(10)))
                .unwrap();
        }
        let text = m.projects_text();
        assert!(text.chars().count() <= PROJECTS_MAX_CHARS + 200);
        assert!(text.contains("[compacted]"));
        assert!(text.contains("事实 199"));
    }

    #[test]
    fn forget_removes_matching_lines_everywhere() {
        let (_d, m) = mem();
        m.add_preference("喜欢用表格").unwrap();
        m.add_preference("在做一个播客").unwrap();
        m.add_project_fact("播客第三期在剪").unwrap();
        let n = m.forget("播客").unwrap();
        assert_eq!(n, 2);
        assert!(!m.profile_text().contains("播客"));
        assert!(!m.projects_text().contains("播客"));
    }

    #[test]
    fn a_blank_needle_refuses_instead_of_erasing_everything() {
        let (_d, m) = mem();
        m.add_preference("喜欢用表格").unwrap();
        m.add_project_fact("播客第三期在剪").unwrap();

        assert!(m.forget("").is_err());
        assert!(m.forget("   ").is_err());
        assert!(m.profile_text().contains("喜欢用表格"));
        assert!(m.projects_text().contains("播客第三期在剪"));
    }
}

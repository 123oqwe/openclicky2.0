use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn resolve(override_root: Option<&Path>) -> Self {
        if let Some(root) = override_root {
            return Self {
                config_dir: root.join("config"),
                data_dir: root.join("data"),
            };
        }
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openclicky");
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openclicky");
        Self {
            config_dir,
            data_dir,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.data_dir.join("logs"))?;
        std::fs::create_dir_all(self.data_dir.join("workspace"))?;
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("app.db")
    }
    pub fn memory_dir(&self) -> PathBuf {
        self.data_dir.join("memory")
    }
    pub fn workspace_dir(&self) -> PathBuf {
        self.data_dir.join("workspace")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.data_dir.join("openclicky.lock")
    }
}

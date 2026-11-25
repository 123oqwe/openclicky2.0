use std::fs::File;
use std::path::Path;

use fs2::FileExt;

use openclicky_core::error::{Error, Result};

pub struct Singleton {
    _file: File,
}

impl Singleton {
    pub fn acquire(lock_path: &Path) -> Result<Self> {
        if let Some(dir) = lock_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|e| Error::Io(e.to_string()))?;
        file.try_lock_exclusive()
            .map_err(|_| Error::State("已经有一个 OpenClicky 在运行".into()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("openclicky.lock");
        let first = Singleton::acquire(&path).unwrap();
        assert!(
            Singleton::acquire(&path).is_err(),
            "second acquire must fail"
        );
        drop(first);
        assert!(
            Singleton::acquire(&path).is_ok(),
            "lock is released on drop"
        );
    }
}

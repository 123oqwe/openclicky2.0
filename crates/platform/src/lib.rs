pub mod audio;
pub mod capture;
pub mod commands;
pub mod http;
pub mod input;
pub mod paths;
pub mod permissions;
pub mod secrets;
pub mod singleton;
pub mod speech;
pub mod tts;

pub use paths::Paths;

/// Construct the secret store without naming its concrete type at call sites.
pub fn open_secrets(
    data_dir: &std::path::Path,
) -> std::sync::Arc<dyn openclicky_core::ports::Secrets> {
    std::sync::Arc::new(secrets::PlatformSecrets::new(data_dir))
}

#[cfg(test)]
mod secrets_tests {
    #[test]
    fn open_secrets_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = super::open_secrets(dir.path());
        let value: openclicky_core::ports::SecretString = "value-1".into();
        store.set("unit/test", &value).unwrap();
        let got = store.get("unit/test").unwrap().unwrap();
        assert_eq!(got.expose(), "value-1");
        store.delete("unit/test").unwrap();
        assert!(store.get("unit/test").unwrap().is_none());
    }
}

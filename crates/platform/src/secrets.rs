//! Key storage: system keychain where available, process memory otherwise.
//!
//! Linux has no keychain here and always runs degraded, so secrets stay in this
//! process and disappear when it exits. Persisting them needs an explicit
//! `opt_in_file_store`.
//!
//! The opt-in file is AES-256-GCM under a key derived from a random salt in its
//! own header. The salt is not a secret and it travels with the file, so the
//! file is obfuscated, not protected: any process running as this user can read
//! it, and a copied file decrypts on another machine. What the random salt does
//! buy is that two installs never share a key, so a key derived on one install
//! says nothing about another. Nothing here defends against same-user access.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

use openclicky_core::error::{Error, Result};
use openclicky_core::ports::{SecretString, Secrets};

const FILE_MAGIC: &[u8; 8] = b"OC2SEC02";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + SALT_LEN + NONCE_LEN;
const KEY_LABEL: &[u8] = b"openclicky-secrets-file-v2";

pub struct PlatformSecrets {
    fallback_path: PathBuf,
    /// Set when the OS keychain is unavailable or failed a read-back.
    degraded: AtomicBool,
    file_store: bool,
    memory: Mutex<HashMap<String, Vec<u8>>>,
}

impl PlatformSecrets {
    pub fn new(data_dir: &Path) -> Self {
        Self::open(data_dir, cfg!(target_os = "linux"))
    }

    fn open(data_dir: &Path, degraded: bool) -> Self {
        std::fs::create_dir_all(data_dir).ok();
        Self {
            fallback_path: data_dir.join("secrets.enc"),
            // Off the keychain the file is only a fallback; degraded means there
            // is no keychain at all, so nothing touches disk until opt-in.
            degraded: AtomicBool::new(degraded),
            // Nothing touches disk until the user opts in.
            file_store: false,
            memory: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    /// True when secrets are memory-only and the caller could still opt in to
    /// writing them to disk. The UI is expected to ask before that happens.
    pub fn needs_opt_in(&self) -> bool {
        self.is_degraded() && !self.file_store
    }

    /// Start persisting to `secrets.enc`, carrying over anything already held in
    /// memory so an opt-in does not lose a key the user just typed.
    pub fn opt_in_file_store(&mut self) -> Result<()> {
        if self.file_store {
            return Ok(());
        }
        let pending = self.memory_snapshot();
        if !pending.is_empty() {
            let mut items = self.read_store();
            for (key, value) in &pending {
                items.retain(|(k, _)| k != key);
                items.push((key.clone(), value.clone()));
            }
            self.write_store(&items)?;
        }
        self.memory().clear();
        self.file_store = true;
        Ok(())
    }

    fn memory(&self) -> MutexGuard<'_, HashMap<String, Vec<u8>>> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn memory_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        self.memory()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn read_store(&self) -> Vec<(String, Vec<u8>)> {
        let Ok(raw) = std::fs::read(&self.fallback_path) else {
            return Vec::new();
        };
        // Anything that is not this format (a file from an older release, a
        // truncated write, someone else's file) reads as empty.
        if raw.len() <= HEADER_LEN || &raw[..FILE_MAGIC.len()] != FILE_MAGIC {
            return Vec::new();
        }
        let salt = &raw[FILE_MAGIC.len()..FILE_MAGIC.len() + SALT_LEN];
        let nonce = &raw[FILE_MAGIC.len() + SALT_LEN..HEADER_LEN];
        let ciphertext = &raw[HEADER_LEN..];
        let Ok(cipher) = Aes256Gcm::new_from_slice(&file_key(salt)) else {
            return Vec::new();
        };
        let Ok(plain) = cipher.decrypt(Nonce::from_slice(nonce), ciphertext) else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_slice::<Vec<(String, String)>>(&plain) else {
            return Vec::new();
        };
        json.into_iter()
            .filter_map(|(k, v)| base64_decode(&v).map(|b| (k, b)))
            .collect()
    }

    fn write_store(&self, items: &[(String, Vec<u8>)]) -> Result<()> {
        let json: Vec<(String, String)> = items
            .iter()
            .map(|(k, v)| (k.clone(), base64_encode(v)))
            .collect();
        let body = serde_json::to_vec(&json).map_err(|e| Error::Internal(e.to_string()))?;

        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&file_key(&salt))
            .map_err(|e| Error::Internal(e.to_string()))?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), body.as_slice())
            .map_err(|_| Error::Internal("encrypt failed".into()))?;

        let mut blob = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        blob.extend_from_slice(FILE_MAGIC);
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);

        let tmp = self.fallback_path.with_extension("enc.tmp");
        write_private(&tmp, &blob)?;
        std::fs::rename(&tmp, &self.fallback_path).map_err(|e| Error::Io(e.to_string()))
    }

    fn file_set(&self, key: &str, value: &str) -> Result<()> {
        let mut items = self.read_store();
        items.retain(|(k, _)| k != key);
        items.push((key.to_string(), value.as_bytes().to_vec()));
        self.write_store(&items)
    }

    fn file_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .read_store()
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| String::from_utf8_lossy(&v).to_string()))
    }

    fn file_delete(&self, key: &str) -> Result<()> {
        let mut items = self.read_store();
        items.retain(|(k, _)| k != key);
        self.write_store(&items)
    }
}

/// Derives the file key from the header salt. Deterministic on purpose: there is
/// no other secret on a machine this code can trust, so the honesty lives in the
/// module comment, not in a fake machine binding.
fn file_key(salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_LABEL);
    hasher.update(salt);
    let out = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out);
    key
}

#[cfg(not(target_os = "linux"))]
mod keychain {
    use openclicky_core::error::{Error, Result};

    pub fn get(key: &str) -> Result<Option<String>> {
        let entry =
            keyring::Entry::new("openclicky", key).map_err(|e| Error::Internal(e.to_string()))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Internal(e.to_string())),
        }
    }

    pub fn set(key: &str, value: &str) -> Result<()> {
        let entry =
            keyring::Entry::new("openclicky", key).map_err(|e| Error::Internal(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| Error::Internal(e.to_string()))
    }

    pub fn delete(key: &str) -> Result<()> {
        let entry =
            keyring::Entry::new("openclicky", key).map_err(|e| Error::Internal(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Internal(e.to_string())),
        }
    }
}

impl Secrets for PlatformSecrets {
    fn get(&self, key: &str) -> Result<Option<SecretString>> {
        #[cfg(not(target_os = "linux"))]
        if !self.is_degraded() {
            if let Ok(Some(v)) = keychain::get(key) {
                return Ok(Some(SecretString::from(v)));
            }
        }
        if let Some(bytes) = self.memory().get(key) {
            return Ok(Some(SecretString::from(
                String::from_utf8_lossy(bytes).to_string(),
            )));
        }
        if self.file_store {
            return Ok(self.file_get(key)?.map(SecretString::from));
        }
        Ok(None)
    }

    fn set(&self, key: &str, value: &SecretString) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        if !self.is_degraded() {
            // A successful write that cannot be read back is not a success.
            let wrote = keychain::set(key, value.expose()).is_ok();
            let read_back = matches!(keychain::get(key), Ok(Some(v)) if v == value.expose());
            if wrote && read_back {
                return Ok(());
            }
            // The keychain did not take it: remember that this install has no
            // usable keychain and keep the secret in memory.
            self.degraded.store(true, Ordering::Relaxed);
        }
        if self.file_store {
            return self.file_set(key, value.expose());
        }
        self.memory()
            .insert(key.to_string(), value.expose().as_bytes().to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        if !self.is_degraded() {
            let _ = keychain::delete(key);
        }
        self.memory().remove(key);
        if self.file_store {
            return self.file_delete(key);
        }
        Ok(())
    }
}

fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| Error::Io(e.to_string()))?;
        file.write_all(body).map_err(|e| Error::Io(e.to_string()))?;
        // The create mode is ANDed with the umask, so restate it.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::Io(e.to_string()))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, body).map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut vals: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        };
        vals.push(v);
    }
    let mut out: Vec<u8> = Vec::new();
    for chunk in vals.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        out.push((chunk[0] << 2) | (chunk[1] >> 4));
        if chunk.len() > 2 {
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if chunk.len() > 3 {
            out.push((chunk[2] << 6) | chunk[3]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_only() -> (tempfile::TempDir, PlatformSecrets) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlatformSecrets::open(dir.path(), true);
        (dir, store)
    }

    fn disk_store() -> (tempfile::TempDir, PlatformSecrets) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PlatformSecrets::open(dir.path(), false);
        store.opt_in_file_store().unwrap();
        (dir, store)
    }

    #[test]
    fn a_degraded_store_keeps_secrets_in_memory_and_writes_no_file() {
        let (dir, store) = memory_only();
        assert!(store.is_degraded());
        assert!(store.needs_opt_in());
        store.set("provider/api_key", &"sk-test".into()).unwrap();
        assert_eq!(
            store.get("provider/api_key").unwrap().unwrap().expose(),
            "sk-test"
        );
        assert!(!dir.path().join("secrets.enc").exists());
        store.delete("provider/api_key").unwrap();
        assert!(store.get("provider/api_key").unwrap().is_none());
    }

    #[test]
    fn opt_in_moves_memory_secrets_to_a_private_file() {
        let (dir, mut store) = memory_only();
        store.set("a", &"one".into()).unwrap();
        store.set("b", &"two".into()).unwrap();
        store.opt_in_file_store().unwrap();
        assert!(!store.needs_opt_in());

        let path = dir.path().join("secrets.enc");
        assert!(path.exists(), "opt-in must create the store file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "store file must be owner-only");
        }

        let mut reopened = PlatformSecrets::open(dir.path(), true);
        reopened.opt_in_file_store().unwrap();
        assert_eq!(reopened.get("a").unwrap().unwrap().expose(), "one");
        assert_eq!(reopened.get("b").unwrap().unwrap().expose(), "two");
    }

    #[test]
    fn an_opt_in_store_deletes_from_both_places() {
        let (_dir, mut store) = memory_only();
        store.set("k", &"value".into()).unwrap();
        store.opt_in_file_store().unwrap();
        store.delete("k").unwrap();
        assert!(store.get("k").unwrap().is_none());
    }

    #[test]
    fn two_installs_never_share_a_file_key() {
        let (dir_a, a) = disk_store();
        let (dir_b, b) = disk_store();
        a.set("k", &"same-value".into()).unwrap();
        b.set("k", &"same-value".into()).unwrap();

        let raw_a = std::fs::read(dir_a.path().join("secrets.enc")).unwrap();
        let raw_b = std::fs::read(dir_b.path().join("secrets.enc")).unwrap();
        let salt_a = &raw_a[FILE_MAGIC.len()..FILE_MAGIC.len() + SALT_LEN];
        let salt_b = &raw_b[FILE_MAGIC.len()..FILE_MAGIC.len() + SALT_LEN];
        assert_ne!(salt_a, salt_b, "each install needs its own random salt");
        assert_ne!(raw_a, raw_b, "same plaintext must not produce same file");
    }

    #[test]
    fn a_corrupt_or_foreign_file_reads_as_empty_instead_of_panicking() {
        let (dir, store) = disk_store();
        store.set("k", &"value".into()).unwrap();
        assert_eq!(store.get("k").unwrap().unwrap().expose(), "value");

        let path = dir.path().join("secrets.enc");
        let mut raw = std::fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&path, &raw).unwrap();
        assert!(store.get("k").unwrap().is_none());

        std::fs::write(&path, b"not our format").unwrap();
        assert!(store.get("k").unwrap().is_none());
    }
}

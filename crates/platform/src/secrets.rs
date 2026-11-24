//! Key storage: system keychain where available, AES-256-GCM file otherwise.
//! The fallback is announced by `doctor`, never silent.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

use openclicky_core::error::{Error, Result};
use openclicky_core::ports::{SecretString, Secrets};

pub struct PlatformSecrets {
    fallback_path: PathBuf,
    degraded: bool,
}

impl PlatformSecrets {
    pub fn new(data_dir: &Path) -> Self {
        std::fs::create_dir_all(data_dir).ok();
        Self {
            fallback_path: data_dir.join("secrets.enc"),
            degraded: cfg!(target_os = "linux"),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded
    }

    fn machine_key(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(hostname().as_bytes());
        hasher.update(whoami().as_bytes());
        hasher.update(b"openclicky-secrets-v1");
        let out = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&out[..32]);
        key
    }

    fn read_store(&self) -> Vec<(String, Vec<u8>)> {
        let Ok(raw) = std::fs::read(&self.fallback_path) else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_slice::<Vec<(String, String)>>(&raw) else {
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
        let tmp = self.fallback_path.with_extension("enc.tmp");
        std::fs::write(&tmp, body).map_err(|e| Error::Io(e.to_string()))?;
        set_private(&tmp)?;
        std::fs::rename(&tmp, &self.fallback_path).map_err(|e| Error::Io(e.to_string()))
    }

    fn file_set(&self, key: &str, value: &str) -> Result<()> {
        let cipher = Aes256Gcm::new_from_slice(&self.machine_key())
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), value.as_bytes())
            .map_err(|_| Error::Internal("encrypt failed".into()))?;
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ciphertext);
        let mut items = self.read_store();
        items.retain(|(k, _)| k != key);
        items.push((key.to_string(), blob));
        self.write_store(&items)
    }

    fn file_get(&self, key: &str) -> Result<Option<String>> {
        let items = self.read_store();
        let Some((_, blob)) = items.into_iter().find(|(k, _)| k == key) else {
            return Ok(None);
        };
        if blob.len() < 13 {
            return Ok(None);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.machine_key())
            .map_err(|e| Error::Internal(e.to_string()))?;
        let (nonce, ct) = blob.split_at(12);
        let plain = cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| {
            Error::Internal("stored secret cannot be decrypted on this machine".into())
        })?;
        Ok(Some(String::from_utf8_lossy(&plain).to_string()))
    }

    fn file_delete(&self, key: &str) -> Result<()> {
        let mut items = self.read_store();
        items.retain(|(k, _)| k != key);
        self.write_store(&items)
    }
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
        if !self.degraded {
            if let Ok(Some(v)) = keychain::get(key) {
                return Ok(Some(SecretString::new(v)));
            }
        }
        Ok(self.file_get(key)?.map(SecretString::new))
    }

    fn set(&self, key: &str, value: &SecretString) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        if !self.degraded {
            // A successful write that cannot be read back is not a success.
            let wrote = keychain::set(key, value.expose()).is_ok();
            let read_back = matches!(keychain::get(key), Ok(Some(v)) if v == value.expose());
            if wrote && read_back {
                return Ok(());
            }
        }
        self.file_set(key, value.expose())
    }

    fn delete(&self, key: &str) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        if !self.degraded {
            let _ = keychain::delete(key);
        }
        self.file_delete(key)
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "localhost".into())
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into())
}

fn set_private(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| Error::Io(e.to_string()))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms).map_err(|e| Error::Io(e.to_string()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
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

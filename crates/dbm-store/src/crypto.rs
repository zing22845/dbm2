use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::paths::{ensure_data_dir, master_key_path};
use crate::{StoreError, StoreResult};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretBox {
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

impl SecretBox {
    pub fn encrypt(plaintext: &str) -> StoreResult<Self> {
        let mut key = load_or_create_master_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| StoreError::Crypto(e.to_string()))?;

        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);

        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        key.zeroize();

        Ok(Self { nonce, ciphertext })
    }

    pub fn decrypt(&self) -> StoreResult<String> {
        let mut key = load_or_create_master_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| StoreError::Crypto(e.to_string()))?;

        let mut plaintext = cipher
            .decrypt(Nonce::from_slice(&self.nonce), self.ciphertext.as_ref())
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let result = std::str::from_utf8(&plaintext)
            .map(|s| s.to_owned())
            .map_err(|e| StoreError::Crypto(e.to_string()));

        plaintext.zeroize();
        key.zeroize();

        result
    }

    pub fn nonce(&self) -> &[u8; NONCE_LEN] {
        &self.nonce
    }

    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn from_parts(nonce: Vec<u8>, ciphertext: Vec<u8>) -> StoreResult<Self> {
        if nonce.len() != NONCE_LEN {
            return Err(StoreError::Crypto(format!(
                "invalid nonce length: {}",
                nonce.len()
            )));
        }
        let mut nonce_arr = [0u8; NONCE_LEN];
        nonce_arr.copy_from_slice(&nonce);
        Ok(Self {
            nonce: nonce_arr,
            ciphertext,
        })
    }
}

fn load_or_create_master_key() -> StoreResult<[u8; KEY_LEN]> {
    ensure_data_dir()?;
    let path = master_key_path()?;
    if path.exists() {
        let mut bytes = std::fs::read(&path)?;
        if bytes.len() != KEY_LEN {
            bytes.zeroize();
            return Err(StoreError::Crypto(format!(
                "master key at {} has invalid length",
                path.display()
            )));
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&bytes);
        bytes.zeroize();
        return Ok(key);
    }

    let mut key = [0u8; KEY_LEN];
    rand::rng().fill_bytes(&mut key);
    crate::paths::ensure_parent(&path)?;
    std::fs::write(&path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_produces_nonce_and_ciphertext() {
        crate::paths::reset_data_dir_for_test();
        let temp = tempfile::tempdir().unwrap();
        crate::paths::init_data_dir(Some(temp.path().to_path_buf())).unwrap();

        let secret = SecretBox::encrypt("secret-pass").unwrap();
        assert_eq!(secret.nonce().len(), NONCE_LEN);
        assert!(!secret.ciphertext().is_empty());

        crate::paths::reset_data_dir_for_test();
    }
}

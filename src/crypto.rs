use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use keyring::Entry;
use rand::RngCore;

const SERVICE: &str = "clipd";
const USER: &str = "history-key";
const NONCE_LEN: usize = 12;

pub struct Crypto {
    cipher: Aes256Gcm,
}

impl Crypto {
    pub fn open() -> Result<Self> {
        let entry = Entry::new(SERVICE, USER).context("keyring entry")?;
        let key_bytes = match entry.get_password() {
            Ok(pw) => B64.decode(pw.trim()).context("decode key from keyring")?,
            Err(keyring::Error::NoEntry) => {
                let mut key = vec![0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                entry
                    .set_password(&B64.encode(&key))
                    .context("store key in KWallet/Secret Service")?;
                key
            }
            Err(e) => return Err(anyhow!("keyring: {e}")),
        };
        if key_bytes.len() != 32 {
            return Err(anyhow!("keyring key must be 32 bytes, got {}", key_bytes.len()));
        }
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    pub fn encrypt(&self, plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plain)
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        Ok((ct, nonce.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        if nonce.len() != NONCE_LEN {
            return Err(anyhow!("bad nonce length {}", nonce.len()));
        }
        self.cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|e| anyhow!("decrypt: {e}"))
    }
}

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use hkdf::Hkdf;
use keyring::Entry;
use rand::RngCore;
use sha2::Sha256;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use zeroize::Zeroizing;

const MASTER_KEY_SERVICE: &str = "toothpaste-desktop";
const MASTER_KEY_USER: &str = "master-key";
const NONCE_SIZE: usize = 12;

/// Encrypted key-value store backed by a single master key in the OS keychain.
///
/// The master key is generated once and persisted in the OS keyring. When an
/// optional password is supplied, the actual cipher key is derived from both
/// the master key and the password via HKDF-SHA256 — decryption then requires
/// possession of both the OS keychain entry and the password.
pub struct StorageService {
    cipher: Aes256Gcm,
    db_path: PathBuf,
    /// In-memory map: key -> base64(nonce || ciphertext || tag)
    db: HashMap<String, String>,
}

impl StorageService {
    /// Load (or initialise) the storage at `db_path`.
    ///
    /// On first run a 32-byte master key is generated and saved to the OS keyring.
    /// If `password` is `Some`, the cipher key is derived from both the master key
    /// and the password using HKDF-SHA256; the same password must be supplied on
    /// every subsequent open or decryption will fail.
    pub fn new(
        db_path: PathBuf, password: Option<&str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = Entry::new(MASTER_KEY_SERVICE, MASTER_KEY_USER)?;

        let master_key: Zeroizing<[u8; 32]> = match entry.get_secret() {
            Ok(key) if key.len() == 32 => Zeroizing::new(key.try_into().unwrap()),
            _ => {
                let mut raw = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut raw);
                entry.set_secret(&raw)?;
                Zeroizing::new(raw)
            }
        };

        // When a password is given, bind it to the master key via HKDF so that
        // both factors are required to reconstruct the cipher key.
        let cipher_key: Zeroizing<[u8; 32]> = Zeroizing::new(match password {
            None => *master_key,
            Some(pw) => {
                let hk = Hkdf::<Sha256>::new(Some(pw.as_bytes()), master_key.as_ref());
                let mut key = [0u8; 32];
                hk.expand(b"toothpaste-desktop-v1", &mut key)
                    .expect("32-byte HKDF output is always valid");
                key
            }
        });

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(cipher_key.as_ref()));

        let db: HashMap<String, String> = if db_path.exists() {
            serde_json::from_str(&fs::read_to_string(&db_path)?)?
        } else {
            HashMap::new()
        };

        Ok(Self {
            cipher,
            db_path,
            db,
        })
    }

    pub fn get(&self, key: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let blob = BASE64.decode(self.db.get(key).ok_or("key not found in storage")?)?;
        if blob.len() < NONCE_SIZE {
            return Err("corrupted storage entry".into());
        }
        let nonce = Nonce::from_slice(&blob[..NONCE_SIZE]);
        self.cipher
            .decrypt(nonce, &blob[NONCE_SIZE..])
            .map_err(|_| "decryption failed".into())
    }

    pub fn set(&mut self, key: &str, value: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut blob = nonce.to_vec();
        blob.extend(
            self.cipher
                .encrypt(&nonce, value)
                .map_err(|_| "encryption failed")?,
        );
        self.db.insert(key.to_string(), BASE64.encode(&blob));
        self.flush()
    }

    pub fn delete(&mut self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db.remove(key);
        self.flush()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.db.contains_key(key)
    }

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.db_path, serde_json::to_string(&self.db)?)?;
        Ok(())
    }
}

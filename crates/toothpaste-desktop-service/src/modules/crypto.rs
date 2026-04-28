use super::storage::StorageService;

use aes_gcm::{
    aead::{Aead, AeadCore, OsRng},
    Aes256Gcm, Key, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hkdf::Hkdf;
use p256::SecretKey;
use p256::PublicKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use rand::thread_rng;


#[derive(Debug)]
pub struct DeviceNotFoundError(pub String);

impl fmt::Display for DeviceNotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no persisted session found for device '{}'", self.0)
    }
}

impl Error for DeviceNotFoundError {}

/// Persisted ECDH key material for a paired device.
/// The whole struct is serialized as JSON and stored encrypted in the db under "session_{device_id}".
/// `aes_key` is excluded from serialization — it is re-derived from the shared secret at runtime.
#[derive(Serialize, Deserialize)]
pub struct EcdhSession {
    device_id: String,
    #[serde(with = "secret_key_serde")]
    self_private_key: SecretKey,
    #[serde(with = "bytes65_serde")]
    self_public_key: [u8; 65],
    #[serde(with = "bytes65_serde")]
    peer_public_key: [u8; 65],
    #[serde(skip)]
    aes_key: Option<Aes256Gcm>,
    #[serde(skip)]
    debug_aes_key_base64: Option<String>,
}

mod secret_key_serde {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use p256::{pkcs8::{DecodePrivateKey, EncodePrivateKey}, SecretKey};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(key: &SecretKey, s: S) -> Result<S::Ok, S::Error> {
        let der = key.to_pkcs8_der().map_err(serde::ser::Error::custom)?;
        BASE64.encode(der.as_bytes()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SecretKey, D::Error> {
        let encoded = String::deserialize(d)?;
        let bytes = BASE64.decode(&encoded).map_err(serde::de::Error::custom)?;
        SecretKey::from_pkcs8_der(&bytes).map_err(serde::de::Error::custom)
    }
}

mod bytes65_serde {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 65], s: S) -> Result<S::Ok, S::Error> {
        BASE64.encode(bytes).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 65], D::Error> {
        let encoded = String::deserialize(d)?;
        let bytes = BASE64.decode(&encoded).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("expected 65 bytes"))
    }
}

/// Main cryptographic handler for ECDH key exchange and AES-GCM encryption/decryption
pub struct EcdhContext {
    /// Active ECDH session for current device
    active_session: Option<EcdhSession>,
    /// Storage backend for persisting keys; sessions are loaded on demand by device_id
    storage: StorageService,
}

impl EcdhContext {
    pub fn new(storage: StorageService) -> Self {
        Self {
            active_session: None,
            storage,
        }
    }

    // ============================================================================
    // Identity Key Management (per device pairing)
    // ============================================================================

    /// Generate and persist a new ECDH key pair for a device pairing.
    /// Call once during initial device pairing — keys persist in keyring.
    pub fn generate_device_keys(&mut self, device_id: &str, peer_public_key: &[u8; 65]) -> Result<[u8; 65], Box<dyn Error>> {
        let private_key = SecretKey::random(&mut thread_rng());
        let pub_key_bytes: [u8; 65] = private_key.public_key()
            .to_encoded_point(false)
            .as_bytes()
            .try_into()?;

        let session = EcdhSession {
            device_id: device_id.to_string(),
            self_private_key: private_key,
            self_public_key: pub_key_bytes,
            peer_public_key: *peer_public_key,
            aes_key: None,
            debug_aes_key_base64: None,
        };

        self.active_session = Some(session);
        Ok(pub_key_bytes)
    }

    /// Load persisted ECDH keys for a device from keyring and derive the AES session key.
    /// Call after initial pairing to restore the device session.
    ///
    /// `challenge_data` is used as HKDF salt; pass `&[]` to use no salt.
    pub fn load_device_keys(&mut self, device_id: &str, challenge_data: &[u8]) -> Result<[u8; 65], Box<dyn Error>> {
        let storage_key = format!("session_{}", device_id);
        if !self.storage.contains(&storage_key) {
            return Err(Box::new(DeviceNotFoundError(device_id.to_string())));
        }

        Self::print_base64_data("Challenge Data", challenge_data);

        let bytes = self.storage.get(&storage_key)?;
        let session: EcdhSession = serde_json::from_slice(&bytes)?;
        let pub_key_bytes = session.self_public_key;
        self.active_session = Some(session);
        let salt = if challenge_data.is_empty() { None } else { Some(challenge_data) };
        self.derive_aes_key(salt)?;
        self.print_aes_key_debug();
        Ok(pub_key_bytes)
    }

    /// Save the current session state back to storage.
    pub fn save_session(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(session) = &self.active_session {
            self.storage.set(
                &format!("session_{}", session.device_id),
                &serde_json::to_vec(session)?,
            )?;
        }
        Ok(())
    }

    /// Switch the active session by loading the given device's keys from storage.
    pub fn set_active_device(&mut self, device_id: &str) -> Result<(), Box<dyn Error>> {
        self.load_device_keys(device_id, &[]).map(|_| ())
    }

    /// Returns true if a persisted session exists for `device_id`.
    pub fn has_session(&self, device_id: &str) -> bool {
        self.storage.contains(&format!("session_{}", device_id))
    }

    /// Returns the stored self public key for `device_id` without loading the full session
    /// or deriving the AES key. Used to send a reconnect greeting before the challenge arrives.
    pub fn get_stored_public_key(&self, device_id: &str) -> Result<[u8; 65], Box<dyn Error>> {
        let bytes = self.storage.get(&format!("session_{}", device_id))?;
        let session: EcdhSession = serde_json::from_slice(&bytes)?;
        Ok(session.self_public_key)
    }

    // ============================================================================
    // Key Compression/Decompression
    // ============================================================================

    /// Decompress a compressed P-256 public key (33 bytes) to uncompressed format (65 bytes).
    pub fn decompress_key(compressed_bytes: &[u8; 33]) -> Result<[u8; 65], Box<dyn Error>> {
        let public_key = PublicKey::from_sec1_bytes(compressed_bytes)
            .map_err(|_| "compressed key is not a valid P-256 point")?;
        Ok(public_key.to_encoded_point(false).as_bytes().try_into()?)
    }

    // ============================================================================
    // Key Derivation
    // ============================================================================

    /// Derive the ECDH shared secret from the active session's private key and peer public key.
    /// Returns 32 raw bytes (the x-coordinate of the shared EC point).
    pub fn derive_shared_secret(&self) -> Result<[u8; 32], Box<dyn Error>> {
        let session = self.active_session.as_ref().ok_or("no active session")?;
        let peer_pub = PublicKey::from_sec1_bytes(&session.peer_public_key)
            .map_err(|_| "invalid peer public key in session")?;
        let shared = p256::ecdh::diffie_hellman(
            session.self_private_key.to_nonzero_scalar(),
            peer_pub.as_affine(),
        );
        let shared_secret: [u8; 32] = shared.raw_secret_bytes().as_slice().try_into()?;

        let shared_secret_hex = shared_secret.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let shared_secret_base64 = BASE64.encode(&shared_secret);
        println!("Shared Secret Generated:");
        println!("  Hex: {}", shared_secret_hex);
        println!("  Base64: {}", shared_secret_base64);

        Ok(shared_secret)
    }

    /// Derive the AES-256-GCM session key via HKDF-SHA256 and store it in the active session.
    /// `salt` is optional HKDF salt — pass `None` to use the empty salt.
    pub fn derive_aes_key(&mut self, salt: Option<&[u8]>) -> Result<(), Box<dyn Error>> {
        let shared_secret = self.derive_shared_secret()?;

        let hk = Hkdf::<Sha256>::new(salt, &shared_secret);
        let mut key_bytes = [0u8; 32];
        hk.expand(b"aes-gcm-256", &mut key_bytes)
            .map_err(|_| "HKDF expand failed")?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        let key_base64 = BASE64.encode(&key_bytes);
        key_bytes.fill(0);

        let session = self.active_session.as_mut().ok_or("no active session")?;
        session.aes_key = Some(cipher);
        session.debug_aes_key_base64 = Some(key_base64);

        Ok(())
    }

    // ============================================================================
    // Encryption/Decryption
    // ============================================================================

    /// Encrypt `data` with the session AES-GCM key.
    /// Returns `nonce(12) || ciphertext || tag(16)`.
    pub fn encrypt_bytes(&self, unencrypted_data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        let aes_key = self.active_session.as_ref()
            .and_then(|s| s.aes_key.as_ref())
            .ok_or("no active AES session key")?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = aes_key
            .encrypt(&nonce, unencrypted_data)
            .map_err(|_| "AES-GCM encryption failed")?;
        let mut out = nonce.to_vec();
        out.extend(ciphertext);
        Ok(out)
    }

    /// Decrypt `nonce(12) || ciphertext || tag(16)` with the session AES-GCM key.
    pub fn decrypt_bytes(&self, ciphertext_bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        let aes_key = self.active_session.as_ref()
            .and_then(|s| s.aes_key.as_ref())
            .ok_or("no active AES session key")?;
        const NONCE_LEN: usize = 12;
        if ciphertext_bytes.len() < NONCE_LEN {
            return Err("ciphertext too short".into());
        }
        let nonce = Nonce::from_slice(&ciphertext_bytes[..NONCE_LEN]);
        aes_key
            .decrypt(nonce, &ciphertext_bytes[NONCE_LEN..])
            .map_err(|_| "AES-GCM decryption failed".into())
    }

    // ============================================================================
    // Key Exchange Flow
    // ============================================================================

    /// Complete pairing flow: decompress peer key, generate our key pair, save session.
    /// Returns our uncompressed public key (65 bytes) to send back to the peer.
    pub fn pair_new_device(&mut self, peer_key: &[u8; 33], device_id: &str)
        -> Result<[u8; 65], Box<dyn Error>> {
        let decompressed_key = EcdhContext::decompress_key(peer_key)?;
        let self_public_key = self.generate_device_keys(device_id, &decompressed_key)?;
        self.save_session()?;
        Ok(self_public_key)
    }

    // ============================================================================
    // Debug helpers
    // ============================================================================

    pub fn get_active_session(&self) -> Option<&EcdhSession> {
        self.active_session.as_ref()
    }

    pub fn clear_session(&mut self) {
        self.active_session = None;
    }

    fn print_base64_data(label: &str, data: &[u8]) {
        let hex_string = data.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let ascii_repr = data.iter()
            .map(|&b| if b >= 32 && b < 127 { b as char } else { '.' })
            .collect::<String>();
        println!("{}: {} bytes", label, data.len());
        println!("  Hex: {}", hex_string);
        println!("  ASCII: {}", ascii_repr);
    }

    pub fn print_aes_key_debug(&self) {
        match self.active_session.as_ref() {
            Some(session) => {
                match &session.debug_aes_key_base64 {
                    Some(key_base64) => {
                        println!("AES-256-GCM Key (Base64): {}", key_base64);
                        println!("AES Key (hex): {}",
                            BASE64.decode(key_base64)
                                .map(|bytes| bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>())
                                .unwrap_or_else(|_| "ERROR".to_string())
                        );
                    },
                    None => eprintln!("AES key not derived yet or derive_aes_key not called"),
                }
            },
            None => eprintln!("No active session"),
        }
    }
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_key_compression_decompression() {
        todo!("Test compression/decompression roundtrip")
    }

    #[test]
    fn test_key_exchange() {
        todo!("Test complete ECDH key exchange")
    }

    #[test]
    fn test_encryption_decryption() {
        todo!("Test AES-GCM encryption/decryption")
    }
}

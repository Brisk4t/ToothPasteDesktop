use crate::storage::StorageService;

use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
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

use toothpaste_desktop_proto::toothpaste::DataPacket;

const ECDH_CURVE: &str = "P-256"; // secp256r1
    
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

    /// Generate and persist a new ECDH key pair for a device pairing
    /// Call once during initial device pairing - keys persist in keyring
    ///
    /// # Arguments
    /// * `device_id` - Unique device identifier (MAC address or device name)
    ///
    /// # Returns
    /// Our public key in uncompressed format (65 bytes: 0x04 + X + Y)
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
        };

        self.active_session = Some(session);
        Ok(pub_key_bytes)
    }

    /// Load persisted ECDH keys for a device from keyring and derive the AES session key.
    /// Call after initial pairing to restore the device session.
    ///
    /// # Arguments
    /// * `device_id` - Device identifier to load keys for
    /// * `challenge_data` - Salt for AES key derivation via HKDF; pass `&[]` to use no salt
    ///
    /// # Errors
    /// Returns `DeviceNotFoundError` if no session exists for `device_id`.
    pub fn load_device_keys(&mut self, device_id: &str, challenge_data: &[u8]) -> Result<[u8; 65], Box<dyn Error>> {
        let storage_key = format!("session_{}", device_id);
        if !self.storage.contains(&storage_key) {
            return Err(Box::new(DeviceNotFoundError(device_id.to_string())));
        }
        
        let bytes = self.storage.get(&storage_key)?;
        let session: EcdhSession = serde_json::from_slice(&bytes)?;
        let pub_key_bytes = session.self_public_key;
        self.active_session = Some(session);
        let salt = if challenge_data.is_empty() { None } else { Some(challenge_data) };
        self.derive_aes_key(salt)?;
        Ok(pub_key_bytes)
    }

    /// Save the current session state back to storage (e.g. after updating peer key or derived AES key)
    pub fn save_session(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(session) = &self.active_session {
            self.storage.set(
                &format!("session_{}", session.device_id),
                &serde_json::to_vec(session)?,
            )?;
        }
        Ok(())
    }

    /// Switch the active session by loading the given device's keys from storage
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

    /// Compress a P-256 public key from uncompressed (65 bytes) to compressed (33 bytes) format
    /// Uses point compression with prefix 0x02 (even Y) or 0x03 (odd Y)
    ///
    /// # Arguments
    /// * `uncompressed_key` - Uncompressed key (65 bytes: 0x04 + X + Y)
    ///
    /// # Returns
    /// Compressed key (33 bytes: prefix + X coordinate)
    pub fn compress_key(uncompressed_key: &[u8; 65]) -> Result<[u8; 33], Box<dyn Error>> {
        todo!("Implement P-256 key compression")
    }

    /// Decompress a compressed P-256 public key (33 bytes) to uncompressed format (65 bytes)
    /// Uses elliptic curve math to recover the full Y coordinate from the compressed format
    ///
    /// # Arguments
    /// * `compressed_bytes` - Compressed public key (33 bytes: prefix + X coordinate)
    ///
    /// # Returns
    /// Uncompressed key (65 bytes: 0x04 + X + Y)
    pub fn decompress_key(compressed_bytes: &[u8; 33]) -> Result<[u8; 65], Box<dyn Error>> {
        let public_key = PublicKey::from_sec1_bytes(compressed_bytes)
            .map_err(|_| "compressed key is not a valid P-256 point")?;
        Ok(public_key.to_encoded_point(false).as_bytes().try_into()?)
    }

    // ============================================================================
    // Key Import/Export
    // ============================================================================

    /// Import a peer's uncompressed public key for ECDH operations
    ///
    /// # Arguments
    /// * `raw_key_buffer` - Raw uncompressed public key (65 bytes)
    ///
    /// # Returns
    /// PublicKey object usable for shared secret derivation
    pub fn import_peer_public_key(raw_key_buffer: &[u8; 65]) -> Result<PublicKey, Box<dyn Error>> {
        todo!("Implement peer public key import")
    }

    /// Import a private key in PKCS8 format for ECDH operations
    ///
    /// # Arguments
    /// * `pkcs8_key_buffer` - Private key in PKCS8 format
    ///
    /// # Returns
    /// SecretKey for key derivation operations
    pub fn import_self_private_key(pkcs8_key_buffer: &[u8]) -> Result<SecretKey, Box<dyn Error>> {
        todo!("Implement self private key import from PKCS8")
    }

    /// Import raw AES-GCM key bytes as a CryptoKey for encryption/decryption
    ///
    /// # Arguments
    /// * `key_bytes` - Raw AES key material (32 bytes for 256-bit key)
    ///
    /// # Returns
    /// Aes256Gcm cipher for encryption/decryption operations
    pub fn import_aes_key_from_bytes(key_bytes: &[u8; 32]) -> Result<Aes256Gcm, Box<dyn Error>> {
        todo!("Implement AES-GCM key import")
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
        Ok(shared.raw_secret_bytes().as_slice().try_into()?)
    }

    /// Derive the AES-256-GCM session key via HKDF-SHA256 and store it in the active session.
    /// The shared secret is derived internally by calling `derive_shared_secret`; it is never
    /// persisted. `salt` is optional HKDF salt — pass `None` to use the empty salt.
    pub fn derive_aes_key(&mut self, salt: Option<&[u8]>) -> Result<(), Box<dyn Error>> {
        let shared_secret = self.derive_shared_secret()?;

        let hk = Hkdf::<Sha256>::new(salt, &shared_secret);
        let mut key_bytes = [0u8; 32];
        hk.expand(b"toothpaste-desktop-aes-v1", &mut key_bytes)
            .map_err(|_| "HKDF expand failed")?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        key_bytes.fill(0); // zeroize before drop

        self.active_session
            .as_mut()
            .ok_or("no active session")?
            .aes_key = Some(cipher);

        Ok(())
    }

    // ============================================================================
    // Encryption/Decryption
    // ============================================================================

    /// Encrypt data using AES-GCM with the derived shared secret key
    /// Generates random 12-byte IV and appends authentication tag (16 bytes)
    ///
    /// # Arguments
    /// * `unencrypted_data` - Data to encrypt
    /// * `aes_key` - AES-GCM cipher
    ///
    /// # Returns
    /// DataPacket (protobuf) containing: IV (12 bytes) + ciphertext + tag (16 bytes)
    pub fn encrypt_text(
        unencrypted_data: &[u8],
        aes_key: &Aes256Gcm,
    ) -> Result<DataPacket, Box<dyn Error>> {
        todo!("Implement AES-GCM encryption")
    }

    /// Decrypt ciphertext using AES-GCM with the derived shared secret key
    ///
    /// # Arguments
    /// * `ciphertext_bytes` - Encrypted data (IV + ciphertext + tag)
    /// * `aes_key` - AES-GCM cipher
    ///
    /// # Returns
    /// Decrypted plaintext as Vec<u8>
    pub fn decrypt_text(
        ciphertext_bytes: &[u8],
        aes_key: &Aes256Gcm,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        todo!("Implement AES-GCM decryption")
    }

    // ============================================================================
    // Key Exchange Flow
    // ============================================================================

    /// Complete key exchange flow: decompress peer key, generate our key pair,
    /// derive AES key, and store all keys in active session
    ///
    /// # Arguments
    /// * `peer_key_compressed` - Peer's compressed public key (33 bytes)
    ///
    /// # Returns
    /// Our uncompressed public key (65 bytes) to send to peer
    pub fn pair_new_device(&mut self, peer_key: &[u8; 33], device_id: &str) 
        -> Result<[u8; 65], Box<dyn Error>> {
        
        let decompressed_key = EcdhContext::decompress_key(peer_key)?;
        
        let self_public_key = self.generate_device_keys(device_id, &decompressed_key)?;
        self.save_session()?;

        Ok(self_public_key)
    }

    /// Get a reference to the active session (for testing/debugging)
    pub fn get_active_session(&self) -> Option<&EcdhSession> {
        self.active_session.as_ref()
    }

    /// Clear the active session
    pub fn clear_session(&mut self) {
        self.active_session = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

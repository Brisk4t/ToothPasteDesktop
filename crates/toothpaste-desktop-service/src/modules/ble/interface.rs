use std::error::Error;
use std::future::Future;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prost::Message;
use tokio::sync::Mutex;
use toothpaste_desktop_proto::packets::{self, create_unencrypted_packet};
use toothpaste_desktop_proto::toothpaste::{data_packet, DataPacket, EncryptedData};

use super::{BleManager, ResponseHandler};
use crate::modules::crypto::EcdhContext;
use crate::modules::storage::StorageService;

// ── Internal ResponseHandler ──────────────────────────────────────────────────
//
// Drives the pairing / auth handshake without exposing ResponseHandler to callers.
// `on_peer_unknown` is the only interactive step; it delegates to a closure
// provided by main so the module stays free of UI concerns.

struct InternalHandler<'a, F> {
    ecdh: &'a Arc<Mutex<EcdhContext>>,
    device_id: &'a str,
    on_peer_unknown: F,
}

impl<'a, F, Fut> ResponseHandler for InternalHandler<'a, F>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Option<[u8; 33]>>,
{
    // Unimplemented 
    async fn on_keepalive(&mut self) -> Option<Vec<u8>> {
        None
    }

    // TODO: Update to only signal an unpaired device instead of immediately entering pairing mode
    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>> {
        let compressed = (self.on_peer_unknown)().await?;
        let pub_key = self.ecdh.lock().await
            .pair_new_device(&compressed, self.device_id)
            .map_err(|e| eprintln!("Pairing failed: {e}"))
            .ok()?;
        Some(create_unencrypted_packet(&BASE64.encode(&pub_key)))
    }

    // TODO: Add firmware check later
    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>> {
        println!("Device recognised (firmware: {firmware_version}). Awaiting challenge...");
        None
    }

    // Derive the session key from the device's challenge and our stored private key.
    async fn on_challenge(&mut self, challenge_data: &[u8]) -> Option<Vec<u8>> {
        let mut ecdh = self.ecdh.lock().await;
        match ecdh.load_device_keys(self.device_id, challenge_data) {
            Ok(_) => println!("Session key derived. Authenticated."),
            Err(e) => eprintln!("Failed to derive session key: {e}"),
        }
        None
    }
}

// ── BLEInterface ─────────────────────────────────────────────────────────────

/// High-level interface that owns the BLE transport and ECDH crypto context.
///
/// After `connect()`, all send methods encrypt the payload with the derived AES-GCM session
/// key and transmit it over BLE — callers only deal in raw data types (strings, coordinates,
/// keycodes). The pairing / authentication handshake is fully contained inside
/// `subscribe_and_handle`; the only thing the caller supplies is a closure that yields the
/// peer's compressed public key when the device signals it is unrecognised.
pub struct BLEInterface {
    ble: BleManager,
    ecdh: Arc<Mutex<EcdhContext>>,
    device_id: Option<String>,
}

impl BLEInterface {
    pub async fn new(storage: StorageService) -> Result<Self, Box<dyn Error>> {
        let ble = BleManager::new().await?;
        let ecdh = Arc::new(Mutex::new(EcdhContext::new(storage)));
        Ok(Self { ble, ecdh, device_id: None })
    }

    // ── Setup ────────────────────────────────────────────────────────────────

    /// Scan for nearby ToothPaste devices and return their names.
    /// Call this first to let the caller display and select a device before connecting.
    pub async fn scan(&mut self) -> Result<Vec<String>, Box<dyn Error>> {
        self.ble.ble_discover_toothpaste().await.map_err(Into::into)
    }

    /// Connect to the named device and (if it is already paired) proactively send our
    /// stored public key so the device can issue an auth challenge immediately.
    pub async fn connect_to_device(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        let mac = self.ble.ble_connect_toothpaste(name).await?;
        self.device_id = Some(mac);

        if self.is_device_known().await {
            if let Err(e) = self.send_public_key().await {
                eprintln!("Failed to send public key: {e}");
            }
        }

        Ok(())
    }

    async fn is_device_known(&self) -> bool {
        let Some(id) = self.device_id.as_deref() else { return false; };
        self.ecdh.lock().await.has_session(id)
    }

    async fn send_public_key(&self) -> Result<(), Box<dyn Error>> {
        let id = self.device_id.as_deref().ok_or("no connected device")?;
        let pub_key = self.ecdh.lock().await.get_stored_public_key(id)?;
        self.ble.ble_send_unencrypted(&BASE64.encode(&pub_key)).await
    }

    // ── Notification loop (fully encapsulated) ────────────────────────────────

    /// Subscribe to device notifications and drive the full pairing/auth/keepalive
    /// handshake internally.
    ///
    /// `get_peer_key` is called only when the device signals it does not recognise us
    /// (`PeerUnknown`). It should return the device's compressed P-256 public key (33 bytes),
    /// or `None` to abort pairing. All other response types are handled automatically.
    ///
    /// Takes `&self` so the concurrent stdin-command branch in the select! loop can also
    /// hold a shared reference and call the `send_*` methods without a borrow conflict.
    pub async fn subscribe_and_handle<F, Fut>(
        &self,
        get_peer_key: F,
    ) -> Result<(), Box<dyn Error>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Option<[u8; 33]>>,
    {
        let id = self.device_id.as_deref().ok_or("no connected device")?;
        let mut handler = InternalHandler {
            ecdh: &self.ecdh,
            device_id: id,
            on_peer_unknown: get_peer_key,
        };
        self.ble.subscribe_notifications(&mut handler).await
    }

    // ── Send helpers (raw data → proto → encrypt → BLE) ──────────────────────

    pub async fn send_keyboard_string(&self, text: &str) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_keyboard_packet(text)).await
    }

    pub async fn send_keycode(&self, code: &[u8]) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_keycode_packet(code)).await
    }

    pub async fn send_mouse(&self, x: f64, y: f64, left: bool, right: bool) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_packet(x, y, left, right)).await
    }

    pub async fn send_mouse_stream(
        &self,
        frames: &[(f64, f64)],
        left: bool,
        right: bool,
        scroll: i32,
    ) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_stream(frames, left, right, scroll)).await
    }

    pub async fn send_rename(&self, name: &str) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_rename_packet(name)).await
    }

    pub async fn send_consumer_control(&self, code: u32) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_consumer_control_packet(code)).await
    }

    pub async fn send_mouse_jiggle(&self, enable: bool) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_jiggle_packet(enable)).await
    }

    // ── Internal ─────────────────────────────────────────────────────────────

    /// Encode `payload` to bytes, encrypt with the session AES-GCM key, wrap in a
    /// `DataPacket`, and transmit over BLE.
    async fn encrypt_and_send(&self, payload: EncryptedData) -> Result<(), Box<dyn Error>> {
        
        // Encode the protobuf message to bytes before encryption.
        let payload_bytes = payload.encode_to_vec();

        // Encrypt the 'EncrpytedData' protobuf message with the session key derived from ECDH.
        let encrypted = {
            let ecdh = self.ecdh.lock().await;
            ecdh.encrypt_bytes(&payload_bytes)?
        };

        // Construct final ToothPaste packet with the encrypted payload inside a DataPacket wrapper.
        // encrypted layout: nonce(12) || ciphertext || tag(16)
        let tag_start = encrypted.len() - 16;
        let packet = DataPacket {
            packet_id: data_packet::PacketId::DataPacket as i32,
            packet_number: 1,
            total_packets: 1,
            slow_mode: true,
            iv: encrypted[..12].to_vec(), // First 12 bytes of the encrypted blob are the nonce (IV).
            data_len: (tag_start - 12) as u32, // Ciphertext length (excluding nonce/IV and tag).
            encrypted_data: encrypted[12..tag_start].to_vec(), // Ciphertext (excluding nonce/IV and tag).
            tag: encrypted[tag_start..].to_vec(), // Last 16 bytes are the authentication tag.
        };

        self.ble.ble_send_encrypted(&packet.encode_to_vec()).await
    }
}

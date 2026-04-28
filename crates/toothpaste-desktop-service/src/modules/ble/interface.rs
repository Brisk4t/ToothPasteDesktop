use std::error::Error;
use std::future::Future;
use std::sync::Arc;
use futures::StreamExt;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prost::Message;
use tokio::sync::Mutex;
use tokio::sync::watch::Sender;
use tokio::sync::mpsc;
use rdev::{Event, EventType, Key};
use toothpaste_desktop_proto::packets::{self, create_unencrypted_packet};
use toothpaste_desktop_proto::toothpaste::{data_packet, DataPacket, EncryptedData};
use toothpaste_desktop_core::{AppState, AuthState, Device, DeviceState};

use super::{BleManager};
use crate::modules::crypto::EcdhContext;
use crate::modules::storage::StorageService;

// ── Internal ResponseHandler ──────────────────────────────────────────────────
//
// Drives the pairing / auth handshake without exposing ResponseHandler to callers.
// `on_peer_unknown` is the only interactive step; it delegates to a closure
// provided by main so the module stays free of UI concerns.

/// Implement this trait to handle incoming `ResponsePacket` notifications from the device.
/// Return `Some(bytes)` to write a packet back over BLE; `None` to send nothing.
pub trait ResponseHandler {
    async fn on_keepalive(&mut self) -> Option<Vec<u8>>;
    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>>;
    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>>;
    async fn on_challenge(&mut self, challenge_data: &[u8], firmware_version: &str) -> Option<Vec<u8>>;
}

struct InternalHandler<'a, F> {
    ecdh: &'a Arc<Mutex<EcdhContext>>,
    device_id: &'a str,
    device_observable: &'a Sender<AppState>,
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
        self.device_observable.send_modify(|d| d.connected_device = Some(Device {
            state: DeviceState::Connected {
                firmware_version: firmware_version.to_string(),
                auth_state: AuthState::Authenticated {
                    pubkey: "N/A".to_string(),
                    session_key: "N/A".to_string(),
                }
            },
            ..d.connected_device.clone().unwrap()
            }));
        println!("Device recognised (firmware: {firmware_version}). Awaiting challenge...");
        None
    }

    // Derive the session key from the device's challenge and our stored private key.
    async fn on_challenge(&mut self, challenge_data: &[u8], firmware_version: &str) -> Option<Vec<u8>> {        
        let mut ecdh = self.ecdh.lock().await;
        match ecdh.load_device_keys(self.device_id, challenge_data) {
            Ok(_) => println!("Session key derived. Authenticated."),
            Err(e) => eprintln!("Failed to derive session key: {e}"),
        }

        // Update device state to authenticated now that the handshake is complete.
        // This will enable encrypted communication in the UI and update the displayed firmware version.
        self.device_observable.send_modify(|d| d.connected_device = Some(Device {
            state: DeviceState::Connected {
                firmware_version: firmware_version.to_string(),
                auth_state: AuthState::Authenticated {
                    pubkey: "N/A".to_string(),
                    session_key: "N/A".to_string(),
                }
            },
            ..d.connected_device.clone().unwrap()
            }));
        None
    }
}

// --- BLEInterface -------------------------------

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
    app_state_channel_tx: Sender<AppState>,
    command_rx: mpsc::Receiver<Event>,
}

impl BLEInterface {
    pub async fn new(storage: StorageService, app_state_channel_tx: Sender<AppState>, command_rx: mpsc::Receiver<Event>) -> Result<Self, Box<dyn Error>> {
        let ble = BleManager::new().await?;
        let ecdh = Arc::new(Mutex::new(EcdhContext::new(storage)));
        Ok(Self { 
            ble, 
            ecdh, 
            device_id: None, 
            app_state_channel_tx, 
            command_rx: command_rx })
    }

    // --- Setup ------------------------------------

    /// Scan for nearby ToothPaste devices by SERVICE_UUID and update app state with discovered devices for the TUI to display.
    /// Call this first to let the caller display and select a device before connecting.
    pub async fn scan(&mut self) -> Result<Vec<Device>, Box<dyn Error>> {
        match self.ble.ble_discover_toothpaste().await {
            Ok(devices) => {
                self.app_state_channel_tx.send_modify(|app_state| {
                    app_state.devices = devices.clone();
                });
                Ok(devices)
            },
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Connect to the named device and (if it is already paired) proactively send our
    /// stored public key so the device can issue an auth challenge immediately.
    pub async fn connect_to_device(&mut self, device: Device) -> Result<(), Box<dyn Error>> {
        let mac = self.ble.ble_connect_toothpaste(device.clone()).await?;
        self.device_id = Some(mac);

        if self.is_device_known().await {
            if let Err(e) = self.send_public_key().await {
                eprintln!("Failed to send public key: {e}");
            }
        }

                    
        self.app_state_channel_tx.send_modify(|app_state| {                
            app_state.connected_device = Some(Device { 
                state: DeviceState::Connected {
                    auth_state: AuthState::NotAuthenticated,
                    firmware_version: "Unknown".to_string(),
                },
                ..device.clone()
            })
        });

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

    // ------------- Main loop ----------------------
    /// This loops forever, processing both notification and command events concurently.
    pub async fn run<F, Fut>(&mut self, get_peer_key: F) -> Result<(), Box<dyn Error>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Option<[u8; 33]>>,
    {
        let id = self.device_id.as_deref().ok_or("no connected device")?;
        let mut handler = InternalHandler {
            ecdh: &self.ecdh,
            device_id: id,
            device_observable: &self.app_state_channel_tx,
            on_peer_unknown: get_peer_key,
        };

        // Subscribe to BLE notifications and get the stream of incoming packets.
        let mut notification_stream = self.ble.subscribe_notifications().await?;

        loop {
            // Ownership is not concurrent, the thread that has an action available first will process it while the other awaits.
            tokio::select! {
                // Await the next BLE notification and dispatch to the appropriate response handler.
                Some(notification) = notification_stream.next() => {
                    if notification.uuid != super::HID_SEMAPHORE_CHARACTERISTIC_UUID {
                        continue;
                    }

                    let packet = match toothpaste_desktop_proto::packets::unpack_response_packet(&notification.value) {
                        Ok(p) => p,
                        Err(e) => { eprintln!("Failed to decode ResponsePacket: {e}"); continue; }
                    };

                    let response = match toothpaste_desktop_proto::toothpaste::response_packet::ResponseType::try_from(packet.response_type) {
                        Ok(toothpaste_desktop_proto::toothpaste::response_packet::ResponseType::Keepalive)   => handler.on_keepalive().await,
                        Ok(toothpaste_desktop_proto::toothpaste::response_packet::ResponseType::PeerUnknown) => handler.on_peer_unknown().await,
                        Ok(toothpaste_desktop_proto::toothpaste::response_packet::ResponseType::PeerKnown)   => handler.on_peer_known(&packet.firmware_version).await,
                        Ok(toothpaste_desktop_proto::toothpaste::response_packet::ResponseType::Challenge)   => handler.on_challenge(&packet.challenge_data, &packet.firmware_version).await,
                        Err(_) => { eprintln!("Unknown response type: {}", packet.response_type); None }
                    };
                    
                    // Response handlers update state (pairing, auth) but don't write back
                    let _ = response;
                }
                
                // Await the next keyboard/mouse event from the channel and send it to the device.
                Some(event) = self.command_rx.recv() => {
                    if let Some(key) = event.name {
                        if let Err(e) = self.send_keyboard_string(key.as_str()).await {
                            eprintln!("Failed to send keyboard event: {e}");
                        }
                    }
                }
            }
        }
    }

    // ------ Send helpers (raw data → proto → encrypt → BLE) -------------------

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

    // ------ Internal ---------------------------------------

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

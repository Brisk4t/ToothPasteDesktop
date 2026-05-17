use futures::StreamExt;
use std::error::Error;
use std::sync::Arc;

use crate::input::handler::InputEvent;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use prost::Message;
use rdev::{Event, EventType, Key};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::watch::Sender;
use toothpaste_desktop_core::{AppCommand, AppState, AuthState, Device, DeviceState};
use toothpaste_desktop_proto::packets;
use toothpaste_desktop_proto::toothpaste::{DataPacket, EncryptedData, data_packet};
use toothpaste_desktop_proto::toothpaste::response_packet::ResponseType;

use super::BleManager;
use crate::modules::crypto::EcdhContext;
use crate::modules::storage::StorageService;

struct InternalHandler<'a> {
    ecdh: &'a Arc<Mutex<EcdhContext>>,
    device_id: &'a str,
    device_observable: &'a Sender<AppState>,
}

impl<'a> InternalHandler<'a> {
    async fn on_keepalive(&mut self) -> Option<Vec<u8>> {
        None
    }

    // Set AuthenticationFailed so the TUI can show pairing UI and send AppCommand::PairDevice.
    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>> {
        self.device_observable.send_modify(|d| {
            if let Some(device) = d.connected_device.clone() {
                let firmware_version = match &device.state {
                    DeviceState::Connected { firmware_version, .. } => firmware_version.clone(),
                    _ => "Unknown".to_string(),
                };
                d.connected_device = Some(Device {
                    state: DeviceState::Connected {
                        auth_state: AuthState::AuthenticationFailed,
                        firmware_version,
                    },
                    ..device
                });
            }
        });
        None
    }

    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>> {
        unimplemented!("Firmware does not implement this, placeholder type")
    }

    // Derive the session key from the device's challenge and our stored private key.
    /// TODO: Implement firmware version check 
    async fn on_challenge(
        &mut self, challenge_data: &[u8], firmware_version: &str,
    ) -> Option<Vec<u8>> {
        let mut ecdh = self.ecdh.lock().await;
        match ecdh.load_device_keys(self.device_id, challenge_data) {
            Ok(_) => println!("Session key derived. Authenticated."),
            Err(e) => eprintln!("Failed to derive session key: {e}"),
        }

        // Update device state to authenticated now that the handshake is complete.
        // This will enable encrypted communication in the UI and update the displayed firmware version.
        self.device_observable.send_modify(|d| {
            d.connected_device = Some(Device {
                state: DeviceState::Connected {
                    firmware_version: firmware_version.to_string(),
                    auth_state: AuthState::Authenticated {
                        pubkey: "N/A".to_string(),
                        session_key: "N/A".to_string(),
                    },
                },
                ..d.connected_device.clone().unwrap()
            })
        });
        None
    }
}

// --- BLEInterface -------------------------------

/// High-level interface that owns the BLE transport and ECDH crypto context.
/// Contains the main, permanent loop that processes incoming BLE notifications and outgoing commands from the TUI / Input Hook process.
pub struct BLEInterface {
    ble: BleManager,
    ecdh: Arc<Mutex<EcdhContext>>,
    device_id: Option<String>,
    app_state_channel_tx: Sender<AppState>,
    command_rx: mpsc::Receiver<InputEvent>,
    pub serial_buffer: Arc<Mutex<Vec<String>>>,
}

impl BLEInterface {
    pub async fn new(
        storage: StorageService, app_state_channel_tx: Sender<AppState>,
        command_rx: mpsc::Receiver<InputEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        let ble = BleManager::new().await?;
        let ecdh = Arc::new(Mutex::new(EcdhContext::new(storage)));
        Ok(Self {
            ble,
            ecdh,
            device_id: None,
            app_state_channel_tx,
            command_rx,
            serial_buffer: Arc::new(Mutex::new(Vec::new())),
        })
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
            }
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
        let Some(id) = self.device_id.as_deref() else {
            return false;
        };
        self.ecdh.lock().await.has_session(id)
    }

    async fn send_public_key(&self) -> Result<(), Box<dyn Error>> {
        let id = self.device_id.as_deref().ok_or("no connected device")?;
        let pub_key = self.ecdh.lock().await.get_stored_public_key(id)?;
        self.ble
            .ble_send_unencrypted(&BASE64.encode(&pub_key))
            .await
    }

    // ------------- Main loop ----------------------

    /// Loops forever, processing BLE notifications, raw input events, and TUI app commands concurrently.
    pub async fn run(
        &mut self, app_command_rx: &mut mpsc::Receiver<AppCommand>,
    ) -> Result<(), String> {
        let id = self.device_id.as_deref().ok_or("no connected device")?;
        let mut handler = InternalHandler {
            ecdh: &self.ecdh,
            device_id: id,
            device_observable: &self.app_state_channel_tx,
        };

        let mut notification_stream = self.ble.subscribe_notifications().await
            .map_err(|e| e.to_string())?;

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

                    let _response = match ResponseType::try_from(packet.response_type) {
                        Ok(ResponseType::Keepalive)   => handler.on_keepalive().await,
                        Ok(ResponseType::PeerUnknown) => handler.on_peer_unknown().await,
                        Ok(ResponseType::PeerKnown)   => handler.on_peer_known(&packet.firmware_version).await,
                        Ok(ResponseType::Challenge)   => handler.on_challenge(&packet.challenge_data, &packet.firmware_version).await,
                        Ok(ResponseType::SerialData)  => {
                            if !packet.serial_data.is_empty() {
                                self.serial_buffer.lock().await.push(packet.serial_data.clone());
                            }
                            None
                        }
                        Err(_) => { eprintln!("Unknown response type: {}", packet.response_type); None }
                    };
                }

                // Process raw input events from the input hook and send appropriate packets to the device.
                Some(event) = self.command_rx.recv() => {
                    match event {
                        InputEvent::RDevEvent(event) => {
                            match event.event_type {
                                EventType::MouseMove{x, y} => {
                                    self.send_mouse(x, y, false, false, 0).await.unwrap_or_else(|e| eprintln!("Failed to send mouse event: {e}"));
                                },
                                EventType::ButtonPress(button) => {
                                    let (left, right) = match button {
                                        rdev::Button::Left => (true, false),
                                        rdev::Button::Right => (false, true),
                                        _ => (false, false),
                                    };
                                    self.send_mouse(0.0, 0.0, left, right, 0).await.unwrap_or_else(|e| eprintln!("Failed to send mouse click event: {e}"));
                                },
                                EventType::ButtonRelease(button) => {
                                    let (left, right) = match button {
                                        rdev::Button::Left => (false, false),
                                        rdev::Button::Right => (false, false),
                                        _ => (false, false),
                                    };
                                    self.send_mouse(0.0, 0.0, left, right, 0).await.unwrap_or_else(|e| eprintln!("Failed to send mouse click release event: {e}"));
                                },
                                EventType::Wheel { delta_x, delta_y } => {
                                    println!("Scroll event sent: delta_x={}, delta_y={}", delta_x, delta_y);
                                    self.send_mouse(0.0, 0.0, false, false, delta_y as i32).await.unwrap_or_else(|e| eprintln!("Failed to send mouse wheel event: {e}"));
                                },
                                EventType::KeyPress(_) | EventType::KeyRelease(_) => {
                                    if let Some(key) = event.name {
                                        self.send_keyboard_string(key.as_str()).await.unwrap_or_else(|e| eprintln!("Failed to send keyboard event: {e}"));
                                    }
                                }
                                _ => { /* Handle other event types if needed */}
                            }
                        }
                        InputEvent::Clipboard(text) => {
                            let sanitized = text
                                .trim()
                                .chars()
                                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                                .collect::<String>();
                            if !sanitized.is_empty() {
                                self.send_keyboard_stream(sanitized.as_str()).await
                                    .unwrap_or_else(|e| eprintln!("Failed to send clipboard text: {e}"));
                            } else {
                                println!("Clipboard text contained no valid ASCII characters");
                            }
                        }
                        InputEvent::Keycode(codes) => {
                            self.send_keycode(&codes).await
                                .unwrap_or_else(|e| eprintln!("Failed to send keycode: {e}"));
                        }
                        InputEvent::ConsumerControl(code) => {
                            self.send_consumer_control(code).await
                                .unwrap_or_else(|e| eprintln!("Failed to send consumer control: {e}"));
                        }
                    }
                }

                Some(cmd) = app_command_rx.recv() => {
                    match cmd {
                        AppCommand::PairDevice { pub_key, .. } => {
                            let compressed: Option<[u8; 33]> = BASE64.decode(&pub_key)
                                .ok()
                                .and_then(|b| b.try_into().ok());
                            if let Some(bytes) = compressed {
                                // Convert Box<dyn Error> to String before any await so it
                                // doesn't appear in the state machine at a yield point.
                                let pair_result: Result<[u8; 65], String> = handler.ecdh
                                    .lock().await
                                    .pair_new_device(&bytes, handler.device_id)
                                    .map_err(|e| e.to_string());
                                match pair_result {
                                    Ok(our_key) => {
                                        if let Err(e) = self.ble.ble_send_unencrypted(&BASE64.encode(&our_key)).await {
                                            eprintln!("Pairing BLE write failed: {e}");
                                        }
                                    }
                                    Err(e) => eprintln!("Pairing failed: {e}"),
                                }
                            } else {
                                eprintln!("PairDevice: pub_key must be a base64-encoded 33-byte compressed P-256 key");
                            }
                        }
                        AppCommand::SendKeyboardInput(text) => {
                            if let Err(e) = self.send_keyboard_string(&text).await {
                                eprintln!("Keyboard send error: {e}");
                            }
                        }
                        AppCommand::SendMouseJiggle(enable) => {
                            if let Err(e) = self.send_mouse_jiggle(enable).await {
                                eprintln!("Mouse jiggle error: {e}");
                            }
                        }
                        AppCommand::KillService => std::process::exit(0),
                        _ => {}
                    }
                }
            }
        }
    }

    // ------ Send helpers (raw data → proto → encrypt → BLE) -------------------

    pub async fn send_keyboard_string(&self, text: &str) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_keyboard_packet(text))
            .await
    }

    pub async fn send_keyboard_stream(&self, text: &str) -> Result<(), Box<dyn Error>> {
        let packets_vec = packets::create_keyboard_stream(text);
        for packet in packets_vec {
            self.encrypt_and_send(packet).await?;
        }
        Ok(())
    }

    pub async fn send_keycode(&self, code: &[u8]) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_keycode_packet(code))
            .await
    }

    pub async fn send_mouse(
        &self, x: f64, y: f64, left: bool, right: bool, wheel: i32,
    ) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_packet(x, y, left, right, wheel))
            .await
    }

    pub async fn send_mouse_stream(
        &self, frames: &[(f64, f64)], left: bool, right: bool, scroll: i32,
    ) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_stream(frames, left, right, scroll))
            .await
    }

    pub async fn send_rename(&self, name: &str) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_rename_packet(name))
            .await
    }

    pub async fn send_consumer_control(&self, code: u32) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_consumer_control_packet(code))
            .await
    }

    pub async fn send_mouse_jiggle(&self, enable: bool) -> Result<(), Box<dyn Error>> {
        self.encrypt_and_send(packets::create_mouse_jiggle_packet(enable))
            .await
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

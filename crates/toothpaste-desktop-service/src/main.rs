use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use notify_rust::Notification;
use prost::Message;
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;
use toothpaste_desktop_proto::packets::{create_keyboard_packet, create_unencrypted_packet};
use toothpaste_desktop_proto::toothpaste::{data_packet, DataPacket};
use toothpaste_desktop_service::crypto::EcdhContext;
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::{BleManager, ResponseHandler};

struct AppHandler {
    ecdh: Arc<Mutex<EcdhContext>>,
    mac_address: String,
}

impl ResponseHandler for AppHandler {
    async fn on_keepalive(&mut self) -> Option<Vec<u8>> {
        None
    }

    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>> {
        println!("Device unknown. Enter peer compressed public key (base64):");
        let input = tokio::task::spawn_blocking(read_line).await.ok()?;
        let decoded = BASE64.decode(input.trim()).ok()?;
        let compressed: [u8; 33] = decoded.try_into().ok()?;
        let mut ecdh = self.ecdh.lock().await;
        match ecdh.pair_new_device(&compressed, &self.mac_address) {
            Ok(our_pub) => Some(create_unencrypted_packet(&BASE64.encode(&our_pub))),
            Err(e) => { eprintln!("Pairing failed: {e}"); None }
        }
    }

    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>> {
        println!("Device recognised (firmware: {firmware_version}). Awaiting challenge...");
        None
    }

    async fn on_challenge(&mut self, challenge_data: &[u8]) -> Option<Vec<u8>> {
        let mut ecdh = self.ecdh.lock().await;
        
        match ecdh.load_device_keys(&self.mac_address, challenge_data) {
            Ok(_) => println!("Session key derived. Authenticated."),
            Err(e) => eprintln!("Failed to derive session key: {e}"),
        }
        None
    }
}

#[tokio::main]
async fn main() {
    let storage_service = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => { eprintln!("Storage init failed: {e}"); return; }
    };

    let mut ble = match BleManager::new().await {
        Ok(b) => b,
        Err(e) => { eprintln!("BLE init failed: {e}"); return; }
    };

    let ecdh = Arc::new(Mutex::new(EcdhContext::new(storage_service)));

    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();

    match ble.ble_discover_toothpaste().await {
        Ok(devices) => { for d in devices { println!("Found: {d}"); } }
        Err(e) => eprintln!("Discovery error: {e}"),
    }

    let mac_address = match ble.ble_connect_toothpaste("ToothPaste-Dev").await {
        Ok(m) => m,
        Err(e) => { eprintln!("Connect failed: {e}"); return; }
    };

    let mut handler = AppHandler { ecdh: Arc::clone(&ecdh), mac_address: mac_address.clone() };

    {
        let ecdh_guard = ecdh.lock().await;
        if ecdh_guard.has_session(&mac_address) {
            match ecdh_guard.get_stored_public_key(&mac_address) {
                Ok(pub_key) => {
                    if let Err(e) = ble.ble_send_unencrypted(&BASE64.encode(&pub_key)).await {
                        eprintln!("Failed to send public key: {e}");
                    }
                }
                Err(e) => eprintln!("Failed to read stored public key: {e}"),
            }
        }
        else {
            drop(ecdh_guard);
            if let Some(packet) = handler.on_peer_unknown().await {
                if let Err(e) = ble.ble_send_unencrypted_packet(&packet).await {
                    eprintln!("Failed to send pairing packet: {e}");
                }
            }
        }
    }

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    tokio::select! {
        result = ble.subscribe_notifications(&mut handler) => {
            if let Err(e) = result { eprintln!("Notification loop error: {e}"); }
        }
        _ = async {
            while let Ok(Some(line)) = lines.next_line().await {
                process_command(&ble, &line, &ecdh).await;
            }
        } => {}
    }
}

async fn process_command(ble: &BleManager, line: &str, ecdh: &Arc<Mutex<EcdhContext>>) {
    match line.trim() {
        "send-pub" => println!("(not yet implemented)"),
        other => send_encrypted_packet(ble, ecdh, other).await,
    }
}

pub fn create_packet(ecdh: &EcdhContext, input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let content_bytes = create_keyboard_packet(input).encode_to_vec();
    let encrypted = ecdh.encrypt_bytes(&content_bytes)?;
    // encrypted = nonce(12) || ciphertext || tag(16)
    let tag_start = encrypted.len() - 16;
    let packet = DataPacket {
        packet_id: data_packet::PacketId::DataPacket as i32,
        packet_number: 1,
        total_packets: 1,
        slow_mode: false,
        iv: encrypted[..12].to_vec(),
        data_len: (tag_start - 12) as u32,
        encrypted_data: encrypted[12..tag_start].to_vec(),
        tag: encrypted[tag_start..].to_vec(),
    };
    Ok(packet.encode_to_vec())
}

async fn send_encrypted_packet(ble: &BleManager, ecdh: &Arc<Mutex<EcdhContext>>, input: &str) {
    let ecdh_guard = ecdh.lock().await;
    match create_packet(&ecdh_guard, input) {
        Ok(bytes) => {
            if let Err(e) = ble.ble_send_encrypted(&bytes).await {
                eprintln!("Failed to send encrypted packet: {e}");
            }
        }
        Err(e) => eprintln!("Failed to create packet: {e}"),
    }
}

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}

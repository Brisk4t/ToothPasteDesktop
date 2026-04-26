use std::io;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use notify_rust::Notification;
use toothpaste_desktop_proto::packets::create_unencrypted_packet;
use toothpaste_desktop_service::crypto::EcdhContext;
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::{BleManager, ResponseHandler};

struct AppHandler {
    ecdh: EcdhContext,
    mac_address: String,
}

impl ResponseHandler for AppHandler {
    async fn on_keepalive(&mut self) -> Option<Vec<u8>> {
        None
    }

    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>> {
        // Device does not recognise us — run the interactive pairing flow.
        println!("Device unknown. Enter peer compressed public key (base64):");
        let input = read_line();
        let decoded = BASE64.decode(input.trim()).ok()?;
        let compressed: [u8; 33] = decoded.try_into().ok()?;
        match self.ecdh.pair_new_device(&compressed, &self.mac_address) {
            Ok(our_pub) => Some(create_unencrypted_packet(&BASE64.encode(&our_pub))),
            Err(e) => { eprintln!("Pairing failed: {e}"); None }
        }
    }

    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>> {
        // Device recognises us; our public key was already sent pre-subscribe.
        // Just wait for the challenge.
        println!("Device recognised (firmware: {firmware_version}). Awaiting challenge...");
        None
    }

    async fn on_challenge(&mut self, challenge_data: &[u8]) -> Option<Vec<u8>> {
        // Derive the shared secret and AES session key, using the challenge as the HKDF salt.
        match self.ecdh.load_device_keys(&self.mac_address, challenge_data) {
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

    let ecdh = EcdhContext::new(storage_service);

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

    // If we have a stored session, proactively send our public key so the device can
    // recognise us and reply with PEER_KNOWN + CHALLENGE. If not, the device will send
    // PEER_UNKNOWN and the handler will drive the pairing flow.
    if ecdh.has_session(&mac_address) {
        match ecdh.get_stored_public_key(&mac_address) {
            Ok(pub_key) => {
                if let Err(e) = ble.ble_send_unencrypted(&BASE64.encode(&pub_key)).await {
                    eprintln!("Failed to send public key: {e}");
                }
            }
            Err(e) => eprintln!("Failed to read stored public key: {e}"),
        }
    }

    let mut handler = AppHandler { ecdh, mac_address };

    if let Err(e) = ble.subscribe_notifications(&mut handler).await {
        eprintln!("Notification loop error: {e}");
    }
}

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}

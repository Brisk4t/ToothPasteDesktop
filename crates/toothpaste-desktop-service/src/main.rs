use std::path::PathBuf;
use std::path::Path;
use std::io::{self, BufRead};

use p256::ecdh;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use toothpaste_desktop_service::BleManager;
use notify_rust::Notification;
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::crypto::EcdhContext;

#[tokio::main]
async fn main() {
    
    let storageLocation: PathBuf = PathBuf::from("toothpaste_storage.json");
    let storageService = match StorageService::new(storageLocation, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error initializing storage: {e}");
            return;
        }
    };
    let mut ecdhContext = EcdhContext::new(storageService);

    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();

    let mut ble = match BleManager::new().await {
        Ok(b) => b,
        Err(e) => { eprintln!("Error: {e}"); return; }
    };

    match ble.ble_discover_toothpaste().await {
        Ok(devices) => {
            for device in devices {
                println!("{device}");
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }

    
    let mac_address = ble.ble_connect_toothpaste("ToothPaste-Dev").await.unwrap();

    let peer_public_key = enter_pairing_mode();
    let decoded = BASE64.decode(peer_public_key.trim()).unwrap();
    let peer_public_key_bytes: [u8; 33] = decoded.try_into().unwrap();

    let self_public_key = ecdhContext.pair_new_device(&peer_public_key_bytes, &mac_address).unwrap();
    let base64_string = BASE64.encode(&self_public_key);

    ble.ble_send_unencrypted(&base64_string).await.unwrap();
}

fn enter_pairing_mode() -> String {
    let stdin = io::stdin();
    let mut input = String::new();
    println!("Enter command (or 'quit' to exit):");
    stdin.read_line(&mut input).unwrap();

    input
}
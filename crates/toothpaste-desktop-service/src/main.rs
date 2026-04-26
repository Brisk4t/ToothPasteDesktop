use std::path::PathBuf;
use std::path::Path;


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

    let self_public_key = ecdhContext.generate_and_persist_device_keys(mac_address.as_str()).unwrap();
    let base64_string = BASE64.encode(&self_public_key);

    ble.ble_send_unencrypted(&base64_string).await.unwrap();
}

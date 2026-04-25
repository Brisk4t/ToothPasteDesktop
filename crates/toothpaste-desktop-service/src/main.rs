use toothpaste_desktop_service::BleManager;
use notify_rust::Notification;

#[tokio::main]
async fn main() {
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

    let mac_address = match ble.ble_connect_toothpaste("ToothPaste-Dev").await {
        Ok(m) => print!("Connected to device with MAC: {m}"),
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    }; 
}

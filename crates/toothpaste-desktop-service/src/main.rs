use toothpaste_desktop_service::ble_scan;
use notify_rust::Notification;

#[tokio::main]
async fn main() {
    
    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();
    
    match ble_scan().await {
        Ok(devices) => {
            for device in devices {
                println!("{device}");
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}

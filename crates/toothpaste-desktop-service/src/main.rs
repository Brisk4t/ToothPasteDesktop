use std::io;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use notify_rust::Notification;
use tokio::io::AsyncBufReadExt;
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::BLEInterface;

#[tokio::main]
async fn main() {
    let storage = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => { eprintln!("Storage init failed: {e}"); return; }
    };

    let mut ble = match BLEInterface::new(storage).await {
        Ok(b) => b,
        Err(e) => { eprintln!("BLE init failed: {e}"); return; }
    };

    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();

    match ble.scan().await {
        Ok(devices) => { for d in &devices { println!("Found: {d}"); } }
        Err(e) => eprintln!("Scan error: {e}"),
    }

    if let Err(e) = ble.connect_to_device("ToothPaste-Dev").await {
        eprintln!("Connect failed: {e}");
        return;
    }

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // Both branches hold &ble (shared ref) — no borrow conflict with select!.
    tokio::select! {
        result = ble.subscribe_and_handle(|| async {
            // Called only when the device signals it does not recognise us (PeerUnknown).
            println!("Device unknown. Enter peer compressed public key (base64):");
            let input = tokio::task::spawn_blocking(read_line).await.ok()?;
            let decoded = BASE64.decode(input.trim()).ok()?;
            decoded.try_into().ok()
        }) => {
            if let Err(e) = result { eprintln!("Notification loop error: {e}"); }
        }
        _ = async {
            while let Ok(Some(line)) = lines.next_line().await {
                process_command(&ble, line.trim()).await;
            }
        } => {}
    }
}

async fn process_command(ble: &BLEInterface, line: &str) {
    match line {
        "send-pub" => println!("(not yet implemented in this mode)"),
        other => {
            if let Err(e) = ble.send_keyboard_string(other).await {
                eprintln!("Send failed: {e}");
            }
        }
    }
}

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}

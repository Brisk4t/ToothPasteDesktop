use std::io;
use std::path::PathBuf;
use rdev::{listen, Event, EventType, Key};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use notify_rust::Notification;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;
use tokio::sync::watch::{Receiver, Sender};
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::BLEInterface;
use toothpaste_desktop_core::{Device, DeviceState};



#[tokio::main]
async fn main() {
    let (device_tx, mut device_rx) = tokio::sync::watch::channel(Device {
        name: "Unknown".to_string(),
        address: "N/A".to_string(),
        id: "N/A".to_string(),
        state: DeviceState::Disconnected,
    });

    // Channel for input events from rdev listener
    let (tx_Fifo, mut command_rx) = mpsc::channel::<Event>(50);
    
    let storage = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => { eprintln!("Storage init failed: {e}"); return; }
    };

    let mut ble = match BLEInterface::new(storage, device_tx, command_rx).await {
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

    match ble.connect_to_device("ToothPaste-Dev").await {
        Ok(_) => {            
            
        }, // Do something on connect
        Err(e) => {
            eprintln!("Connect failed: {e}");
            return;
        }
    }

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let result = ble.subscribe_and_handle(|| async {
        // Called only when the device signals it does not recognise us (PeerUnknown).
        println!("Device unknown. Enter peer compressed public key (base64):");
        let input = tokio::task::spawn_blocking(read_line).await.ok()?;
        let decoded = BASE64.decode(input.trim()).ok()?;
        decoded.try_into().ok()
    }); 
    
    // Only continue if subscription was successful, otherwise the device will disconnect immediately and we don't want to start the event loop.
    match result.await {
        Ok(_) => {
            while !matches!(device_rx.borrow().state, DeviceState::Connected{..}) {
                println!("Waiting for device to connect...");
            }
            println!("Device connected, starting event loop...");
            tokio::spawn(async move {
                ble.wait_for_command().await;
                // while let Ok(Some(line)) = lines.next_line().await {
                //     process_command(&ble, line.trim(), &device_rx).await;
                // }
            });
            // Attach rdev listener
            listen(move |event| {
                match tx_Fifo.try_send(event) {
                    Ok(_) => {},
                    Err(e) => eprintln!("Failed to send event: {e}"),
                };
            }).unwrap();
        },
            Err(e) => eprintln!("Subscription failed: {e}"),
    }

}

// Testing only, will be extracted or drastically changed later.
async fn process_command(ble: &BLEInterface, line: &str, device_rx: &Receiver<Device>) {
    match line {
        "get-state" => {
                let device = device_rx.borrow();
                let firmware = match &device.state {
                    DeviceState::Connected { firmware_version, .. } => firmware_version,
                    _ => "N/A",
                };
                println!("Connected to device: {}, firmware version: {:?}", device.name, firmware);

        },
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

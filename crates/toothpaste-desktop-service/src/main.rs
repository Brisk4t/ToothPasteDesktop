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
use toothpaste_desktop_core::{Device, DeviceState, AppState, AuthState};



#[tokio::main]
async fn main() {

    let (app_state_tx, mut app_state_rx) = tokio::sync::watch::channel(AppState {
        app_version: "0.1.0".to_string(),
        app_string: "ToothPaste Desktop Service".to_string(),
        devices: Vec::new(),
        auto_connect: None,
        connected_device: None,
        password_protected: false,
    });


    // // The device state is shared between the service and TUI 
    // let (device_tx, mut device_rx) = tokio::sync::watch::channel(Device {
    //     name: "Unknown".to_string(),
    //     address: "N/A".to_string(),
    //     id: "N/A".to_string(),
    //     signal_strength: -100,
    //     state: DeviceState::Disconnected,
    // });

    // Channel for input events from rdev listener
    let (tx_fifo, mut command_rx) = mpsc::channel::<Event>(50);
    
    // Initialize storage exiting on failure.
    let storage = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => { eprintln!("Storage init failed: {e}"); return; }
    };

    // Initialize BLE interface exiting on failure.
    let mut ble = match BLEInterface::new(storage, app_state_tx.clone(), command_rx).await {
        Ok(b) => b,
        Err(e) => { eprintln!("BLE init failed: {e}"); return; }
    };

    // Show a desktop notification to indicate the service is running.
    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();

    // Synchronized state of the application, shared between the BLE service and the TUI
    // let mut app_state = AppState {
    //     app_version: "0.1.0".to_string(),
    //     app_string: "ToothPaste Desktop Service".to_string(),
    //     devices: Vec::new(),
    //     auto_connect: None,
    //     connected_device: None,
    //     password_protected: false,
    // };

    // Start BLE scanning and print discovered devices.
    // Updates the shared app state with the list of discovered devices for the TUI to display.
    match ble.scan().await {
        Ok(devices) => { 
            app_state_tx.send_modify(|app_state| {
                app_state.devices = devices.clone();
            });
            for d in &devices { 
            println!("Found: {}, address: {}, id: {}",d.name, d.address, d.id); 
        }}
        Err(e) => eprintln!("Scan error: {e}"),
    }

    // Attempt to connect to the device, exiting on failure.
    // TODO: This should be triggered by the TUI when the user selects a device OR
    // TODO: If a save state exists, this should be attempted for a saved device if it exists in the scan results.
    let selected_device = app_state_rx.borrow().devices.get(0).cloned().unwrap(); // For testing, just select the first device found
    match ble.connect_to_device(selected_device.clone()).await {
        Ok(_) => {            
            app_state_tx.send_modify(|app_state| {                
                app_state.connected_device = Some(Device { 
                    state: DeviceState::Connected {
                        auth_state: AuthState::NotAuthenticated,
                        firmware_version: "Unknown".to_string(),
                    },
                    ..selected_device.clone()
                })
            });
        },
        Err(e) => {
            eprintln!("Connect failed: {e}");
            return;
        }
    }

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // Spawn the BLE event loop to handle notifications and commands
    // On PeerUnknown, it will prompt the user for input and send the response back to the device.
    // TODO: The pairing flow should be move to the TUI, this is just for testing.
    tokio::spawn(async move {
        if let Err(e) = ble.run(|| async {
            // Called only when the device signals it does not recognise us (PeerUnknown).
            println!("Device unknown. Enter peer compressed public key (base64):");
            let input = tokio::task::spawn_blocking(read_line).await.ok()?;
            let decoded = BASE64.decode(input.trim()).ok()?;
            decoded.try_into().ok()
        }).await {
            eprintln!("BLE loop error: {e}");
        }
    });

    // Spawn rdev listener thread to capture keyboard/mouse events
    // This blocks main perpetually
    listen(move |event| {
        match tx_fifo.try_send(event) {
            Ok(_) => {},
            Err(e) => eprintln!("Failed to queue event: {e}"),
        };
    }).ok();

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

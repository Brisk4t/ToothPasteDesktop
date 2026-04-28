use std::io;
use std::path::PathBuf;
use rdev::{listen, Event, EventType, Key};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use notify_rust::Notification;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use toothpaste_desktop_service::storage::StorageService;
use toothpaste_desktop_service::BLEInterface;
use toothpaste_desktop_core::{Device, DeviceState, AppState, AuthState, AppCommand};



#[tokio::main]
async fn main() {
    // ------- Channels ------------------------

    // Shared application state channel used by the BLE interface and the TUI. 
    // Updated by the BLE interface when devices are discovered/connected and by the TUI when the user selects a device to connect to or changes settings.
    let (app_state_tx, app_state_rx) = tokio::sync::watch::channel(
        AppState {
            app_version: "0.1.0".to_string(),
            app_string: "ToothPaste Desktop Service".to_string(),
            devices: Vec::new(),
            auto_connect: None,
            connected_device: None,
            password_protected: false,
        }
    );

    // Channel for input events from rdev listener
    // TODO: Since its shared by keyboard and mouse, might need a larger buffer or debounce / consume mouse events quicker
    let (input_event_tx, input_event_rx) = mpsc::channel::<Event>(50);
    let (tui_tx, mut tui_rx) = mpsc::channel::<AppCommand>(10);
    
    // Initialize storage exiting on failure.
    let storage = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => { eprintln!("Storage init failed: {e}"); return; }
    };

    // Initialize BLE interface exiting on failure.
    let mut ble = match BLEInterface::new(storage, app_state_tx.clone(), input_event_rx).await {
        Ok(b) => b,
        Err(e) => { eprintln!("BLE init failed: {e}"); return; }
    };

    // Show a desktop notification to indicate the service is running.
    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running in the background...")
        .show()
        .unwrap();


    // ------- TUI Command Handler (with timeout fallback) ----------------

    tokio::select! {
        Some(command) = tui_rx.recv() => {
            handle_tui_command(&mut ble, &app_state_rx, command).await;
        },
        
        _ = sleep(Duration::from_secs(2)) => {
            // TUI not attached within 2 seconds, fallback triggered
            println!("TUI not attached, fallback triggered");
            match ble.scan().await {
                Ok(devices) => { 
                    app_state_tx.send_modify(|app_state| {
                        app_state.devices = devices.clone();
                    });
                    for d in &devices { 
                        println!("Found: {}, address: {}, id: {}",d.name, d.address, d.id); 
                    }
                }
                Err(e) => eprintln!("Scan error: {e}"),
            }

            let selected_device = app_state_rx.borrow().devices.get(0).cloned().unwrap();
            match ble.connect_to_device(selected_device.clone()).await {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Connect failed: {e}");
                    return;
                }
            }
        },
    }
    

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

    // Spawn background task to listen for TUI commands continuously
    tokio::spawn(async move {
        while let Some(command) = tui_rx.recv().await {
            println!("Received TUI command: {:?}", command);
            // TODO: Process commands here or route to BLE interface via a separate channel
        }
    });

    // Spawn rdev listener thread to capture keyboard/mouse events
    // This blocks main perpetually
    listen(move |event| {
        match input_event_tx.try_send(event) {
            Ok(_) => {},
            Err(e) => eprintln!("Failed to queue event: {e}"),
        };
    }).ok();

}

async fn handle_tui_command(ble: &mut BLEInterface, app_state_rx: &tokio::sync::watch::Receiver<AppState>, command: AppCommand) {
    match command {
        AppCommand::ScanForDevices => {
            if let Err(e) = ble.scan().await {
                eprintln!("Scan failed: {e}");
            }
        },
        AppCommand::ConnectToDevice(device) => {
            if let Err(e) = ble.connect_to_device(device).await {
                eprintln!("Connect failed: {e}");
            }
        },
        _ => println!("Received command: {:?}", command)
    }
}

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}

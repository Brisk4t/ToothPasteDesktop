use std::path::PathBuf;

use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Listener, Stream},
    traits::tokio::Listener as _,
    traits::tokio::Stream as _,
};
use notify_rust::Notification;
use rdev::listen;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, Duration};
use toothpaste_desktop_core::{AppCommand, AppState, IPC_SOCKET_NAME, IpcMessage, SETTINGS_FILE_DEFAULT_PATH};
use toothpaste_desktop_service::{BLEInterface, storage::StorageService};
use std::fs;

#[tokio::main]
async fn main() {

    // Channels --------------------------------------------------------------------------------

    // App state observable, owned by this service, watched by TUI
    let (app_state_tx, app_state_rx) = watch::channel(AppState {
        app_version: "0.1.0".to_string(),
        app_string: "ToothPaste Desktop Service".to_string(),
        devices: Vec::new(),
        auto_connect: None,
        connected_device: None,
        password_protected: false,
        settings_file_path: Some(SETTINGS_FILE_DEFAULT_PATH.to_string()),
    });

    // Command channel for any commands sent to the BLE task (i.e. to the ToothPaste device)
    let (input_event_tx, input_event_rx) = mpsc::channel::<rdev::Event>(50);

    // Command channel for any commands sent from the TUI to the service (e.g. scan, connect, send input, etc.)
    let (app_command_tx, app_command_rx) = mpsc::channel::<AppCommand>(32);

    // tui_connected: Track whether the TUI client is connected
    let (tui_connected_tx, tui_connected_rx) = watch::channel(false);

    // Initialize services ----------------------------------------------------------------------
    // Get settings file path from app state (or use default) and initialize storage
    let settings_file_path = app_state_rx.borrow()
        .settings_file_path
        .clone()
        .unwrap_or_else(|| SETTINGS_FILE_DEFAULT_PATH.to_string());
    
    let db_path = PathBuf::from(&settings_file_path);
    
    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("Failed to create settings directory: {e}");
                return;
            }
        }
    }

    // Initialize storage
    let storage = match StorageService::new(db_path, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Storage init failed: {e}");
            return;
        }
    };

    // Initialize BLE Interface
    let ble = match BLEInterface::new(storage, app_state_tx.clone(), input_event_rx).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("BLE init failed: {e}");
            return;
        }
    };

    // Send a notification to indicate the service is running
    Notification::new()
        .summary("ToothPaste Desktop Service")
        .body("Service is running...")
        .show()
        .ok();


    // Spawn BLE and IPC tasks --------------------------------------------------------------------------------
    tokio::spawn(ble_task(ble, app_command_rx));
    tokio::spawn(ipc_server(
        app_state_rx.clone(),
        app_command_tx.clone(),
        app_state_tx,
        tui_connected_tx,
    ));
    tokio::spawn(auto_connect_task(
        app_state_rx,
        app_command_tx,
        tui_connected_rx,
    ));

    // Attach the blocking listener for global input events. This will run indefinitely until the program exits.
    listen(move |event| {
        let _ = input_event_tx.try_send(event);
    })
    .ok();
}

// ── BLE task ──────────────────────────────────────────────────────────────────

async fn ble_task(mut ble: BLEInterface, mut app_command_rx: mpsc::Receiver<AppCommand>) {
    while let Some(cmd) = app_command_rx.recv().await {
        match cmd {
            AppCommand::ScanForDevices => {
                if let Err(e) = ble.scan().await {
                    eprintln!("Scan error: {e}");
                }
            }
            AppCommand::ConnectToDevice(device) => {
                if let Err(e) = ble.connect_to_device(device).await {
                    eprintln!("Connect error: {e}");
                    continue;
                }
                if let Err(e) = ble.run(&mut app_command_rx).await {
                    eprintln!("BLE run error: {e}");
                }
            }
            AppCommand::SendKeyboardInput(text) => {
                if let Err(e) = ble.send_keyboard_string(&text).await {
                    eprintln!("Keyboard send error: {e}");
                }
            }
            AppCommand::SendMouseJiggle(enable) => {
                if let Err(e) = ble.send_mouse_jiggle(enable).await {
                    eprintln!("Mouse jiggle error: {e}");
                }
            }
            AppCommand::UpdateSettings {
                auto_connect: _,
                password_protected: _,
                settings_file_path,
            } => {
                // Note: Settings updates are handled in ipc_server for app state updates
                // This task just logs them for now
                if settings_file_path.is_some() {
                    println!("Settings file path update requested (handle in ipc_server)");
                }
            }
            AppCommand::KillService => {
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

// ── IPC server ────────────────────────────────────────────────────────────────

async fn ipc_server(
    app_state_rx: watch::Receiver<AppState>, app_command_tx: mpsc::Sender<AppCommand>,
    app_state_tx: watch::Sender<AppState>, tui_connected_tx: watch::Sender<bool>,
) {
    let name = match IPC_SOCKET_NAME.to_ns_name::<GenericNamespaced>() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("IPC name error: {e}");
            return;
        }
    };
    let listener: Listener = match ListenerOptions::new().name(name).create_tokio() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("IPC listen failed: {e}");
            return;
        }
    };
    println!("IPC server listening on socket '{IPC_SOCKET_NAME}'");

    loop {
        match listener.accept().await {
            Ok(stream) => {
                println!("TUI connected");
                let _ = tui_connected_tx.send(true);
                handle_connection(
                    stream,
                    app_state_rx.clone(),
                    app_command_tx.clone(),
                    app_state_tx.clone(),
                )
                .await;
                println!("TUI disconnected");
                let _ = tui_connected_tx.send(false);
            }
            Err(e) => eprintln!("IPC accept error: {e}"),
        }
    }
}

async fn handle_connection(
    stream: Stream, mut app_state_rx: watch::Receiver<AppState>,
    app_command_tx: mpsc::Sender<AppCommand>, _app_state_tx: watch::Sender<AppState>,
) {
    let (recv, mut send) = stream.split();
    let mut lines = BufReader::new(recv).lines();

    let initial = app_state_rx.borrow().clone();
    let _ = send_msg(&mut send, &IpcMessage::State(initial)).await;

    loop {
        tokio::select! {
            result = app_state_rx.changed() => {
                if result.is_err() { break; }
                let state = app_state_rx.borrow_and_update().clone();
                if send_msg(&mut send, &IpcMessage::State(state)).await.is_err() { break; }
            }

            line = lines.next_line() => {
                match line {
                    Ok(Some(text)) => {
                        if let Ok(msg) = serde_json::from_str::<IpcMessage>(&text) {
                            match msg {
                                IpcMessage::Command(cmd) => { app_command_tx.send(cmd).await.ok(); }
                                _ => {}
                            }
                        }
                    }
                    _ => break,
                }
            }
        }
    }
}

async fn send_msg<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &IpcMessage) -> Result<(), ()> {
    let mut json = serde_json::to_string(msg).map_err(|_| ())?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await.map_err(|_| ())
}

// ── Auto-connect task ──────────────────────────────────────────────────────────

// Fallback logic to auto-connect to the first available device if TUI doesn't connect within 5 seconds of service startup.
async fn auto_connect_task(
    mut app_state_rx: watch::Receiver<AppState>,
    app_command_tx: mpsc::Sender<AppCommand>,
    tui_connected_rx: watch::Receiver<bool>,
) {
    // Wait 2 seconds before checking if TUI is connected
    sleep(Duration::from_secs(5)).await;

    // If TUI is already connected, don't auto-connect
    if *tui_connected_rx.borrow() {
        println!("TUI is connected, skipping auto-connect");
        return;
    }

    println!("TUI not connected after 5s, initiating auto-scan and connect");

    // Send scan command
    if let Err(e) = app_command_tx.send(AppCommand::ScanForDevices).await {
        eprintln!("Failed to send scan command: {e}");
        return;
    }

    // Wait for devices to be found (with a timeout)
    let start = tokio::time::Instant::now();
    let scan_timeout = Duration::from_secs(10);

    loop {
        if app_state_rx.changed().await.is_err() {
            break;
        }

        let device_to_connect = {
            let state = app_state_rx.borrow();
            state.devices.first().cloned()
        };

        if let Some(device) = device_to_connect {
            // Found devices, connect to the first one
            println!("Auto-connecting to first device: {:?}", device);
            if let Err(e) = app_command_tx.send(AppCommand::ConnectToDevice(device)).await {
                eprintln!("Failed to send connect command: {e}");
            }
            break;
        }

        // Check if we've exceeded the scan timeout
        if start.elapsed() > scan_timeout {
            println!("Auto-scan timeout, no devices found");
            break;
        }
    }
}

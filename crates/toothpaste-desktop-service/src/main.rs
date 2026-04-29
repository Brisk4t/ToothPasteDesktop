use std::path::PathBuf;
use std::sync::Arc;

use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Listener, Stream},
    traits::tokio::Listener as _,
    traits::tokio::Stream as _,
};
use notify_rust::Notification;
use rdev::listen;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::time::{sleep, Duration};
use toothpaste_desktop_core::{AppCommand, AppState, IPC_SOCKET_NAME, IpcMessage};
use toothpaste_desktop_service::{BLEInterface, storage::StorageService};

#[tokio::main]
async fn main() {

    // Channels --------------------------------------------------------------------------------
    let (app_state_tx, app_state_rx) = watch::channel(AppState {
        app_version: "0.1.0".to_string(),
        app_string: "ToothPaste Desktop Service".to_string(),
        devices: Vec::new(),
        auto_connect: None,
        connected_device: None,
        password_protected: false,
    });

    let (input_event_tx, input_event_rx) = mpsc::channel::<rdev::Event>(50);
    let (app_command_tx, app_command_rx) = mpsc::channel::<AppCommand>(32);

    // pair_req: BLE signals the IPC server that the device needs pairing.
    let (pair_req_tx, pair_req_rx) = mpsc::channel::<()>(1);
    // pair_resp: IPC server forwards the TUI's answer back to the BLE run loop.
    let (pair_resp_tx, pair_resp_rx) = mpsc::channel::<[u8; 33]>(1);

    // tui_connected: Track whether the TUI client is connected
    let (tui_connected_tx, tui_connected_rx) = watch::channel(false);

    let pair_req_rx = Arc::new(Mutex::new(pair_req_rx));
    let pair_resp_rx = Arc::new(Mutex::new(pair_resp_rx));

    // Initialize services ----------------------------------------------------------------------
    // Initialize storage
    let storage = match StorageService::new(PathBuf::from("toothpaste_storage.json"), None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Storage init failed: {e}");
            return;
        }
    };

    // Initialize BLE
    let ble = match BLEInterface::new(storage, app_state_tx, input_event_rx).await {
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


    // Spawn ble and IPC tasks --------------------------------------------------------------------------------
    tokio::spawn(ble_task(ble, app_command_rx, pair_req_tx, pair_resp_rx));
    tokio::spawn(ipc_server(
        app_state_rx.clone(),
        app_command_tx.clone(),
        pair_req_rx,
        pair_resp_tx,
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

async fn ble_task(
    mut ble: BLEInterface, mut app_command_rx: mpsc::Receiver<AppCommand>, pair_req_tx: mpsc::Sender<()>,
    pair_resp_rx: Arc<Mutex<mpsc::Receiver<[u8; 33]>>>,
) {
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
                let req_tx = pair_req_tx.clone();
                let resp_rx = pair_resp_rx.clone();
                
                if let Err(e) = ble.run(|| {
                    let tx = req_tx.clone();
                    let rx = resp_rx.clone();
                    async move {
                        tx.send(()).await.ok();
                        rx.lock().await.recv().await
                    }
                })
                .await
                {
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
            AppCommand::KillService => {
                eprintln!(">>> KillService COMMAND RECEIVED <<<");
                std::io::Write::flush(&mut std::io::stderr()).ok();
                std::io::Write::flush(&mut std::io::stdout()).ok();
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

// ── IPC server ────────────────────────────────────────────────────────────────

async fn ipc_server(
    app_state_rx: watch::Receiver<AppState>, app_command_tx: mpsc::Sender<AppCommand>,
    pair_req_rx: Arc<Mutex<mpsc::Receiver<()>>>, pair_resp_tx: mpsc::Sender<[u8; 33]>,
    tui_connected_tx: watch::Sender<bool>,
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
                    pair_req_rx.clone(),
                    pair_resp_tx.clone(),
                    tui_connected_tx.clone(),
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
    stream: Stream, mut app_state_rx: watch::Receiver<AppState>, app_command_tx: mpsc::Sender<AppCommand>,
    pair_req_rx: Arc<Mutex<mpsc::Receiver<()>>>, pair_resp_tx: mpsc::Sender<[u8; 33]>,
    _tui_connected_tx: watch::Sender<bool>,
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

            pair = async { pair_req_rx.lock().await.recv().await } => {
                if pair.is_none() { break; }
                if send_msg(&mut send, &IpcMessage::PairRequest).await.is_err() { break; }
            }

            line = lines.next_line() => {
                match line {
                    Ok(Some(text)) => {
                        if let Ok(msg) = serde_json::from_str::<IpcMessage>(&text) {
                            match msg {
                                IpcMessage::Command(cmd) => { 
                                    eprintln!("[Service IPC] Received command: {:?}", cmd);
                                    if let Err(e) = app_command_tx.send(cmd).await {
                                        eprintln!("[Service IPC] Failed to send command to ble_task: {}", e);
                                    }
                                }
                                IpcMessage::PairResponse(bytes) => {
                                    if let Ok(arr) = bytes.try_into() {
                                        pair_resp_tx.send(arr).await.ok();
                                    }
                                }
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

    println!("TUI not connected after 2s, initiating auto-scan and connect");

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

mod tui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use interprocess::local_socket::{
    GenericNamespaced, ToNsName, tokio::Stream, traits::tokio::Stream as _,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState, IPC_SOCKET_NAME, IpcMessage};

#[tokio::main]
async fn main() -> io::Result<()> {
    let stream = connect_or_spawn().await?;

    let (cmd_tx, cmd_rx) = mpsc::channel::<AppCommand>(32);

    let (app_state_rx, service_available_rx) = spawn_ipc_bridge(stream, cmd_rx);

    tokio::task::block_in_place(|| tui::start_tui(app_state_rx, service_available_rx, cmd_tx))
}

// ── IPC bridge ────────────────────────────────────────────────────────────────

fn spawn_ipc_bridge(
    stream: Stream,
    cmd_rx: mpsc::Receiver<AppCommand>,
) -> (watch::Receiver<AppState>, watch::Receiver<bool>) {
    let (app_state_tx, app_state_rx) = watch::channel(AppState::default());
    let (service_available_tx, service_available_rx) = watch::channel(true);
    tokio::spawn(ipc_bridge(stream, app_state_tx, service_available_tx, cmd_rx));
    (app_state_rx, service_available_rx)
}

async fn ipc_bridge(
    initial_stream: Stream,
    app_state_tx: watch::Sender<AppState>,
    service_available_tx: watch::Sender<bool>,
    mut cmd_rx: mpsc::Receiver<AppCommand>,
) {
    let mut pending = Some(initial_stream);

    loop {
        let stream = match pending.take() {
            Some(s) => s,
            None => {
                service_available_tx.send(false).ok();
                app_state_tx.send(AppState::default()).ok();
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    match try_connect().await {
                        Ok(s) => break s,
                        Err(_) => continue,
                    }
                }
            }
        };

        service_available_tx.send(true).ok();

        let (recv, mut send) = stream.split();
        let mut lines = BufReader::new(recv).lines();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            if let Ok(IpcMessage::State(s)) = serde_json::from_str(&text) {
                                app_state_tx.send(s).ok();
                            }
                        }
                        _ => break, // connection closed; outer loop will reconnect
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    if write_msg(&mut send, &IpcMessage::Command(cmd)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

async fn write_msg<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &IpcMessage) -> Result<(), ()> {
    let mut json = serde_json::to_string(msg).map_err(|_| ())?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await.map_err(|_| ())
}

// ── Service discovery / spawn ─────────────────────────────────────────────────

async fn connect_or_spawn() -> io::Result<Stream> {
    if let Ok(s) = try_connect().await {
        return Ok(s);
    }

    spawn_service();

    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Ok(s) = try_connect().await {
            return Ok(s);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::ConnectionRefused,
        "Could not connect to toothpaste-desktop-service",
    ))
}

pub async fn try_connect() -> io::Result<Stream> {
    let name = IPC_SOCKET_NAME
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    Stream::connect(name).await
}

pub fn spawn_service() {
    let mut path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_owned()))
        .unwrap_or_else(|| PathBuf::from("."));

    path.push("toothpaste-desktop-service");
    #[cfg(windows)]
    path.set_extension("exe");

    std::process::Command::new(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok();
}

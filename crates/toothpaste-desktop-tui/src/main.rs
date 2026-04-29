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
    let (pair_req_tx, pair_req_rx) = mpsc::channel::<()>(1);
    let (pair_resp_tx, pair_resp_rx) = mpsc::channel::<[u8; 33]>(1);

    let app_state_rx = spawn_ipc_bridge(stream, cmd_rx, pair_req_tx, pair_resp_rx);

    tokio::task::block_in_place(|| tui::start_tui(app_state_rx, cmd_tx, pair_req_rx, pair_resp_tx))
}

// ── IPC bridge ────────────────────────────────────────────────────────────────
//
// Owns the watch::Sender internally. Returns only the Receiver so the rest of
// the TUI never holds a write handle to the app state.

fn spawn_ipc_bridge(
    stream: Stream, cmd_rx: mpsc::Receiver<AppCommand>, pair_req_tx: mpsc::Sender<()>,
    pair_resp_rx: mpsc::Receiver<[u8; 33]>,
) -> watch::Receiver<AppState> {
    let (app_state_tx, app_state_rx) = watch::channel(AppState::default());
    tokio::spawn(ipc_bridge(
        stream,
        app_state_tx,
        cmd_rx,
        pair_req_tx,
        pair_resp_rx,
    ));
    app_state_rx
}

async fn ipc_bridge(
    stream: Stream, app_state_tx: watch::Sender<AppState>, mut cmd_rx: mpsc::Receiver<AppCommand>,
    pair_req_tx: mpsc::Sender<()>, mut pair_resp_rx: mpsc::Receiver<[u8; 33]>,
) {
    let (recv, mut send) = stream.split();
    let mut lines = BufReader::new(recv).lines();

    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(text)) => match serde_json::from_str::<IpcMessage>(&text) {
                        Ok(IpcMessage::State(s)) => { app_state_tx.send(s).ok(); }
                        Ok(IpcMessage::PairRequest) => { pair_req_tx.send(()).await.ok(); }
                        _ => {}
                    },
                    _ => break,
                }
            }

            Some(cmd) = cmd_rx.recv() => {
                if write_msg(&mut send, &IpcMessage::Command(cmd)).await.is_err() { break; }
            }

            Some(arr) = pair_resp_rx.recv() => {
                if write_msg(&mut send, &IpcMessage::PairResponse(arr.to_vec())).await.is_err() { break; }
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

async fn try_connect() -> io::Result<Stream> {
    let name = IPC_SOCKET_NAME
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    Stream::connect(name).await
}

fn spawn_service() {
    let mut path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_owned()))
        .unwrap_or_else(|| PathBuf::from("."));

    path.push("toothpaste-desktop-service");
    #[cfg(windows)]
    path.set_extension("exe");

    std::process::Command::new(&path).spawn().ok();
}

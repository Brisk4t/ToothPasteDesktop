use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::{Extension, Json, Router, response::Response, routing::post};
use rdev::{Button, Event, EventType};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use toothpaste_desktop_core::{AppCommand, AppState, Device, DeviceState};

use crate::modules::input::handler::InputEvent;

pub const MCP_PORT: u16 = 7465;

#[derive(Clone)]
struct McpState {
    app_command_tx: mpsc::Sender<AppCommand>,
    input_event_tx: mpsc::Sender<InputEvent>,
    app_state_rx: watch::Receiver<AppState>,
}

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

pub async fn run_mcp_server(
    app_command_tx: mpsc::Sender<AppCommand>,
    input_event_tx: mpsc::Sender<InputEvent>,
    app_state_rx: watch::Receiver<AppState>,
) {
    let state = Arc::new(McpState { app_command_tx, input_event_tx, app_state_rx });

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .layer(Extension(state));

    let addr = format!("127.0.0.1:{MCP_PORT}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => { eprintln!("MCP server bind failed on {addr}: {e}"); return; }
    };
    println!("MCP server listening on http://{addr}/mcp");

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("MCP server error: {e}");
    }
}

async fn handle_mcp(
    Extension(state): Extension<Arc<McpState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    use axum::response::IntoResponse;

    // Notifications (no id) — acknowledge with empty 200
    let Some(id) = req.id else {
        return axum::http::StatusCode::OK.into_response();
    };

    let result: Result<Value, Value> = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "ToothPaste Desktop", "version": "0.1.0" }
        })),
        "tools/list" => Ok(tools_list()),
        "tools/call" => match req.params {
            Some(p) => tools_call(state, p).await,
            None => Err(json!({"code": -32602, "message": "params required"})),
        },
        _ => Err(json!({"code": -32601, "message": "Method not found"})),
    };

    let body = match result {
        Ok(r)  => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    };
    Json(body).into_response()
}

fn tools_list() -> Value {
    json!({ "tools": [
        {
            "name": "connect",
            "description": "Scan for ToothPaste devices and connect to the first one found.",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        },
        {
            "name": "disconnect",
            "description": "Disconnect from the currently connected ToothPaste device.",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        },
        {
            "name": "get_status",
            "description": "Get the current connection status and device information.",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        },
        {
            "name": "type_text",
            "description": "Type a string as keyboard input on the connected device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to type" }
                },
                "required": ["text"]
            }
        },
        {
            "name": "press_key",
            "description": "Press a key or key combination (e.g. \"Return\", \"ctrl+c\", \"alt+tab\", \"shift+f5\").",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name or combo separated by '+'" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "mouse_move",
            "description": "Move the mouse cursor by a relative offset.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "Horizontal delta (positive = right)" },
                    "y": { "type": "number", "description": "Vertical delta (positive = down)" }
                },
                "required": ["x", "y"]
            }
        },
        {
            "name": "mouse_click",
            "description": "Click a mouse button on the connected device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "button": {
                        "type": "string",
                        "enum": ["left", "right", "middle"],
                        "description": "Button to click (default: left)"
                    }
                },
                "required": []
            }
        },
        {
            "name": "mouse_scroll",
            "description": "Scroll the mouse wheel on the connected device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "delta": { "type": "integer", "description": "Scroll amount (positive = down, negative = up)" }
                },
                "required": ["delta"]
            }
        },
        {
            "name": "media_control",
            "description": "Control media playback or system audio/brightness on the connected device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["play_pause", "next_track", "prev_track", "volume_up", "volume_down", "mute_toggle", "brightness_up", "brightness_down"],
                        "description": "Media action to perform"
                    }
                },
                "required": ["action"]
            }
        }
    ]})
}

// params is taken by value (owned) to avoid borrowing across awaits in handle_mcp
async fn tools_call(state: Arc<McpState>, params: Value) -> Result<Value, Value> {
    let name = params["name"].as_str()
        .ok_or_else(|| json!({"code": -32602, "message": "missing tool name"}))?
        .to_string();
    // Clone arguments as owned strings to avoid &str across awaits
    let args = params["arguments"].clone();

    match name.as_str() {
        "connect"       => tool_connect(state).await,
        "disconnect"    => tool_disconnect(state).await,
        "get_status"    => Ok(tool_get_status(&state)),
        "type_text"     => {
            let text = args["text"].as_str()
                .ok_or_else(|| json!({"code": -32602, "message": "missing text"}))?
                .to_string();
            tool_type_text(state, text).await
        }
        "press_key"     => {
            let key = args["key"].as_str()
                .ok_or_else(|| json!({"code": -32602, "message": "missing key"}))?
                .to_string();
            tool_press_key(state, key).await
        }
        "mouse_move"    => {
            let x = args["x"].as_f64().ok_or_else(|| json!({"code": -32602, "message": "missing x"}))?;
            let y = args["y"].as_f64().ok_or_else(|| json!({"code": -32602, "message": "missing y"}))?;
            tool_mouse_move(state, x, y).await
        }
        "mouse_click"   => {
            let button = args["button"].as_str().unwrap_or("left").to_string();
            tool_mouse_click(state, button).await
        }
        "mouse_scroll"  => {
            let delta = args["delta"].as_i64()
                .ok_or_else(|| json!({"code": -32602, "message": "missing delta"}))?;
            tool_mouse_scroll(state, delta as i32).await
        }
        "media_control" => {
            let action = args["action"].as_str()
                .ok_or_else(|| json!({"code": -32602, "message": "missing action"}))?
                .to_string();
            tool_media_control(state, action).await
        }
        _ => Err(json!({"code": -32602, "message": format!("unknown tool: {name}")})),
    }
}

// ── Tool implementations ─────────────────────────────────────────────────────

async fn tool_connect(state: Arc<McpState>) -> Result<Value, Value> {
    // Extract what we need from state without holding the guard across any await.
    // watch::Ref<'_, AppState> holds an RwLockReadGuard which is !Send, so it must
    // not exist at any suspend point in the future.
    let (connected_msg, cached_device) = {
        let s = state.app_state_rx.borrow();
        let msg = s.connected_device.as_ref().map(|d| format!("Already connected to {}", d.name));
        let dev = s.devices.first().cloned();
        (msg, dev)
        // guard dropped here
    };

    if let Some(msg) = connected_msg {
        return Ok(tool_ok(msg));
    }
    if let Some(dev) = cached_device {
        return do_connect(state, dev).await;
    }

    state.app_command_tx.send(AppCommand::ScanForDevices).await
        .map_err(|_| json!({"code": -32603, "message": "service channel closed"}))?;

    // Wait up to 12 s for a device to appear
    let mut rx = state.app_state_rx.clone();
    let device = timeout(Duration::from_secs(12), async move {
        loop {
            rx.changed().await.ok();
            // Use a block so the Ref guard is dropped before the next await
            let maybe = { rx.borrow().devices.first().cloned() };
            if let Some(d) = maybe { return d; }
        }
    }).await.map_err(|_| json!({"code": -32603, "message": "No devices found within timeout"}))?;

    do_connect(state, device).await
}

async fn do_connect(state: Arc<McpState>, device: Device) -> Result<Value, Value> {
    let name = device.name.clone();
    state.app_command_tx.send(AppCommand::ConnectToDevice(device)).await
        .map_err(|_| json!({"code": -32603, "message": "service channel closed"}))?;

    let mut rx = state.app_state_rx.clone();
    timeout(Duration::from_secs(10), async move {
        loop {
            rx.changed().await.ok();
            let connected = { rx.borrow().connected_device.is_some() };
            if connected { return; }
        }
    }).await.map_err(|_| json!({"code": -32603, "message": "Connection timed out"}))?;

    Ok(tool_ok(format!("Connected to {name}")))
}

async fn tool_disconnect(state: Arc<McpState>) -> Result<Value, Value> {
    state.app_command_tx.send(AppCommand::DisconnectDevice).await
        .map_err(|_| json!({"code": -32603, "message": "service channel closed"}))?;
    Ok(tool_ok("Disconnected"))
}

fn tool_get_status(state: &Arc<McpState>) -> Value {
    // Non-async: the guard is created and dropped within this call, no awaits.
    let (connected, device_name, firmware, app_version, key_capture, clipboard_capture) = {
        let s = state.app_state_rx.borrow();
        let connected = s.connected_device.is_some();
        let device_name = s.connected_device.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "none".to_string());
        let firmware = s.connected_device.as_ref()
            .and_then(|d| match &d.state {
                DeviceState::Connected { firmware_version, .. } => Some(firmware_version.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "unknown".to_string());
        (connected, device_name, firmware, s.app_version.clone(), s.enable_key_capture, s.enable_clipboard_capture)
    };

    let status = json!({
        "connected": connected,
        "deviceName": device_name,
        "firmwareVersion": firmware,
        "appVersion": app_version,
        "keyCaptureEnabled": key_capture,
        "clipboardCaptureEnabled": clipboard_capture,
    });
    json!({
        "content": [{ "type": "text", "text": status.to_string() }],
        "isError": false
    })
}

async fn tool_type_text(state: Arc<McpState>, text: String) -> Result<Value, Value> {
    let display = text.clone();
    state.app_command_tx.send(AppCommand::SendKeyboardInput(text)).await
        .map_err(|_| json!({"code": -32603, "message": "service channel closed"}))?;
    Ok(tool_ok(format!("Typed: {display}")))
}

async fn tool_press_key(state: Arc<McpState>, key: String) -> Result<Value, Value> {
    let report = parse_key_combo(&key)
        .ok_or_else(|| json!({"code": -32602, "message": format!("unrecognised key: {key}")}))?;
    state.input_event_tx.send(InputEvent::Keycode(report.to_vec())).await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    Ok(tool_ok(format!("Pressed: {key}")))
}

async fn tool_mouse_move(state: Arc<McpState>, x: f64, y: f64) -> Result<Value, Value> {
    state.input_event_tx.send(InputEvent::RDevEvent(make_event(EventType::MouseMove { x, y }))).await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    Ok(tool_ok(format!("Mouse moved ({x}, {y})")))
}

async fn tool_mouse_click(state: Arc<McpState>, button: String) -> Result<Value, Value> {
    let btn = match button.as_str() {
        "right"  => Button::Right,
        "middle" => Button::Middle,
        _        => Button::Left,
    };
    state.input_event_tx.send(InputEvent::RDevEvent(make_event(EventType::ButtonPress(btn)))).await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    state.input_event_tx.send(InputEvent::RDevEvent(make_event(EventType::ButtonRelease(btn)))).await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    Ok(tool_ok(format!("{button} click")))
}

async fn tool_mouse_scroll(state: Arc<McpState>, delta: i32) -> Result<Value, Value> {
    state.input_event_tx
        .send(InputEvent::RDevEvent(make_event(EventType::Wheel { delta_x: 0, delta_y: delta as i64 })))
        .await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    Ok(tool_ok(format!("Scrolled {delta}")))
}

async fn tool_media_control(state: Arc<McpState>, action: String) -> Result<Value, Value> {
    // USB HID Consumer Control usage codes
    let code: u32 = match action.as_str() {
        "play_pause"      => 0xCD,
        "next_track"      => 0xB5,
        "prev_track"      => 0xB6,
        "volume_up"       => 0xE9,
        "volume_down"     => 0xEA,
        "mute_toggle"     => 0xE2,
        "brightness_up"   => 0x6F,
        "brightness_down" => 0x70,
        _ => return Err(json!({"code": -32602, "message": format!("unknown action: {action}")})),
    };
    state.input_event_tx.send(InputEvent::ConsumerControl(code)).await
        .map_err(|_| json!({"code": -32603, "message": "input channel closed"}))?;
    Ok(tool_ok(format!("Media: {action}")))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn tool_ok(msg: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": msg.into() }], "isError": false })
}

fn make_event(event_type: EventType) -> Event {
    Event { time: SystemTime::now(), name: None, event_type }
}

/// Parse a key combo string like "ctrl+shift+t" into an 8-byte HID report.
///
/// Report layout (Arduino Keyboard library encoding used by the firmware):
///   bytes 0-4: modifier codes (0x80=LCtrl, 0x81=LShift, 0x82=LAlt, 0x83=LMeta)
///   byte  5  : main key code (ASCII for printable chars, special codes for others)
///   bytes 6-7: reserved (0x00)
fn parse_key_combo(key_str: &str) -> Option<[u8; 8]> {
    let parts: Vec<&str> = key_str.split('+').collect();
    let mut report = [0u8; 8];
    let mut mod_idx = 0usize;

    let (mods, main) = parts.split_at(parts.len().saturating_sub(1));

    for m in mods {
        let code: u8 = match m.to_lowercase().as_str() {
            "ctrl" | "control" => 0x80,
            "shift"            => 0x81,
            "alt"              => 0x82,
            "meta" | "win" | "cmd" | "gui" => 0x83,
            _ => return None,
        };
        if mod_idx < 5 { report[mod_idx] = code; mod_idx += 1; }
    }

    report[5] = key_name_to_code(main.first()?)?;
    Some(report)
}

fn key_name_to_code(name: &str) -> Option<u8> {
    match name.to_lowercase().as_str() {
        "return" | "enter"             => Some(0xB0),
        "backspace"                    => Some(0xB2),
        "tab"                          => Some(0xB3),
        "escape" | "esc"               => Some(0xB1),
        "delete" | "del"               => Some(0xD4),
        "insert"                       => Some(0xD1),
        "home"                         => Some(0xD2),
        "end"                          => Some(0xD5),
        "pageup"   | "pgup"            => Some(0xD3),
        "pagedown" | "pgdn" | "pgdown" => Some(0xD6),
        "up"                           => Some(0xDA),
        "down"                         => Some(0xD9),
        "left"                         => Some(0xD8),
        "right"                        => Some(0xD7),
        "f1"  => Some(0xC2), "f2"  => Some(0xC3), "f3"  => Some(0xC4),
        "f4"  => Some(0xC5), "f5"  => Some(0xC6), "f6"  => Some(0xC7),
        "f7"  => Some(0xC8), "f8"  => Some(0xC9), "f9"  => Some(0xCA),
        "f10" => Some(0xCB), "f11" => Some(0xCC), "f12" => Some(0xCD),
        "space" => Some(0x20),
        s if s.len() == 1 => {
            let c = s.chars().next()?;
            if c.is_ascii() { Some(c as u8) } else { None }
        }
        _ => None,
    }
}

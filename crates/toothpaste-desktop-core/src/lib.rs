use serde::{Deserialize, Serialize};

/// Name used for the local socket (named pipe on Windows, Unix socket on Linux/macOS).
pub const IPC_SOCKET_NAME: &str = "toothpaste-desktop";
pub const APP_VERSION: &str = "0.1.0";
pub const APP_STRING: &str = "Toothpaste Desktop";
pub const SETTINGS_FILE_DEFAULT_PATH: &str = "F:\\VSCode\\ClipBoard\\ToothPasteDesktop\\crates\\toothpaste-desktop-service\\toothpaste_storage.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AuthState {
    NotAuthenticated,
    AuthenticationFailed,
    Authenticated { pubkey: String, session_key: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeviceState {
    Connected {
        auth_state: AuthState,
        firmware_version: String,
    },
    Disconnected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub address: String,
    pub id: String,
    pub state: DeviceState,
    pub signal_strength: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppState {
    pub app_version: String,
    pub app_string: String,
    pub devices: Vec<Device>,
    pub auto_connect: Option<Device>,
    pub connected_device: Option<Device>,
    pub password_protected: bool,
    pub settings_file_path: Option<String>,
    pub enable_key_capture: bool,
    pub enable_clipboard_capture: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppCommand {
    ScanForDevices,
    ConnectToDevice(Device),
    DisconnectDevice,
    PairDevice {
        device: Device,
        pub_key: String,
    },
    SendKeyboardInput(String),
    SendMouseJiggle(bool),
    UpdateSettings {
        auto_connect: Option<Device>,
        password_protected: bool,
        settings_file_path: Option<String>,
    },
    KillService,
}

/// Wire protocol between the service and TUI.
/// Each message is serialised as a single JSON line terminated by `\n`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum IpcMessage {
    /// Service → TUI: full AppState snapshot on every change.
    State(AppState),
    /// TUI → Service: a command to execute.
    Command(AppCommand),
}

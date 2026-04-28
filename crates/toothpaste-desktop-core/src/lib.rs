
#[derive(Clone, Debug)]
pub enum AuthState {
    NotAuthenticated,
    AuthenticationFailed,
    Authenticated {
        pubkey: String,
        session_key: String,
    },
}

#[derive(Clone, Debug)]
pub enum DeviceState {
    Connected{
        auth_state: AuthState,
        firmware_version: String,
    },
    Disconnected,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub address: String,
    pub id: String,
    pub state: DeviceState,
    pub signal_strength: i32,
}

#[derive(Debug)]
pub struct AppState {
    pub app_version: String,
    pub app_string: String,
    pub devices: Vec<Device>,
    pub auto_connect: Option<Device>, // Optional device to auto-connect to if found during scanning
    pub connected_device: Option<Device>, // Currently connected device
    pub password_protected: bool, // Whether the local storage is password protected
}

#[derive(Clone, Debug)]
pub enum AppCommand {
    ScanForDevices,
    ConnectToDevice(Device), // Device ID or address
    DisconnectDevice,
    PairDevice{
        device: Device,
        pub_key: String,
    },
    SendKeyboardInput(String),
    SendMouseJiggle(bool), // Enable or disable mouse jiggle
    UpdateSettings {
        auto_connect: Option<Device>,
        password_protected: bool,
        settings_file_path: Option<String>,
    },
}
pub enum AuthState {
    NotAuthenticated,
    AuthenticationFailed,
    Authenticated {
        pubkey: String,
        session_key: String,
    },
}

pub enum DeviceState {
    Connected{
        auth_state: AuthState,
        signal_strength: i32,
        firmware_version: String,
    },
    Disconnected,
}

pub struct Device {
    pub name: String,
    pub address: String,
    pub id: String,
    pub state: DeviceState,
}


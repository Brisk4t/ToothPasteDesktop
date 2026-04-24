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

struct Device {
    name: String,
    address: String,
    id: String,
    state: DeviceState,
}


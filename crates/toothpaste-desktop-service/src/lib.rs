pub mod modules;

// Flat re-exports for convenient access by the binary and any external consumers.
pub use modules::ble::BleManager;
pub use modules::ble::interface::BLEInterface;
pub use modules::crypto;
pub use modules::mcp;
pub use modules::storage;
pub use modules::input;
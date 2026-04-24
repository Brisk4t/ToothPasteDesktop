use std::time::Duration;
use tokio::time;

use btleplug::api::{Central, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::Manager;

pub async fn ble_scan() -> Result<Vec<String>, btleplug::Error> {
    let manager = Manager::new().await?;
    let adapter_list = manager.adapters().await?;

    if adapter_list.is_empty() {
        eprintln!("No Bluetooth adapters found");
        return Ok(Vec::new());
    }

    let mut discovered_devices = Vec::new();
    for adapter in adapter_list.iter() {
        adapter.start_scan(ScanFilter::default()).await.expect("Failed to start scan");
        time::sleep(Duration::from_secs(5)).await;

        let peripherals = adapter.peripherals().await?;
        for peripheral in peripherals.iter() {
            let properties = peripheral.properties().await?;
            if let Some(local_name) = properties.unwrap().local_name {
                discovered_devices.push(local_name);
            }
        }
    }

    Ok(discovered_devices)
}

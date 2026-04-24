use std::time::Duration;
use tokio::time;
use uuid::{uuid, Uuid};

use btleplug::api::{Central, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::Manager;

const SERVICE_UUID: Uuid                    = uuid!("19b10000-e8f2-537e-4f6c-d104768a1214");
const PACKET_CHARACTERISTIC_UUID: Uuid      = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5907");
const HID_SEMAPHORE_CHARACTERISTIC_UUID: Uuid = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5908");
const MAC_ADDRESS_CHARACTERISTIC_UUID: Uuid = uuid!("19b10002-e8f2-537e-4f6c-d104768a1214");

pub async fn ble_scan() -> Result<Vec<String>, btleplug::Error> {
    let manager = Manager::new().await?;
    let adapter_list = manager.adapters().await?;

    if adapter_list.is_empty() {
        eprintln!("No Bluetooth adapters found");
        return Ok(Vec::new());
    }

    let mut discovered_devices = Vec::new();
    for adapter in adapter_list.iter() {
        // Directly filtering by service UUID doesn't work reliably
        adapter.start_scan(ScanFilter::default()).await.expect("Failed to start scan");
        time::sleep(Duration::from_secs(5)).await;

        let peripherals = adapter.peripherals().await?;
        for peripheral in peripherals.iter() {
            let properties = peripheral.properties().await?;
            if let Some(props) = properties {
                if props.services.contains(&SERVICE_UUID) {
                    discovered_devices.push(props.local_name.unwrap_or_default());
                }
            }
        }
    }

    Ok(discovered_devices)
}

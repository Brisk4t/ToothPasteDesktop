use std::time::Duration;
use tokio::time;
use uuid::{uuid, Uuid};
use toothpaste_desktop_core::{Device, DeviceState};
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};

pub mod interface;

const SERVICE_UUID: Uuid                      = uuid!("19b10000-e8f2-537e-4f6c-d104768a1214");
const PACKET_CHARACTERISTIC_UUID: Uuid        = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5907");
const HID_SEMAPHORE_CHARACTERISTIC_UUID: Uuid = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5908");
const MAC_ADDRESS_CHARACTERISTIC_UUID: Uuid   = uuid!("19b10002-e8f2-537e-4f6c-d104768a1214");


struct CachedPeripheral {
    packet_char: Characteristic,
    semaphore_char: Characteristic,
    #[allow(dead_code)]
    mac_char: Characteristic,
}

pub struct BleManager {
    manager: Manager,
    adapter: Adapter,
    found_peripherals: Vec<Peripheral>,
    connected_peripherial: Option<Peripheral>,
    cached_peripheral: Option<CachedPeripheral>,
}

impl BleManager {
    pub async fn new() -> Result<Self, btleplug::Error> {
        let manager = Manager::new().await?;
        let adapter_list = manager.adapters().await?;

        if adapter_list.is_empty() {
            eprintln!("No Bluetooth adapters found");
            return Err(btleplug::Error::Other("No Bluetooth adapters found".into()));
        }

        Ok(Self {
            manager,
            adapter: adapter_list.into_iter().next().unwrap(),
            found_peripherals: Vec::new(),
            connected_peripherial: None,
            cached_peripheral: None,
        })
    }

    pub async fn ble_discover_toothpaste(&mut self) -> Result<Vec<Device>, btleplug::Error> {
        let mut discovered_devices: Vec<Device> = Vec::new();
        self.adapter.start_scan(ScanFilter::default()).await.expect("Failed to start scan");
        time::sleep(Duration::from_secs(5)).await;

        let peripherals = self.adapter.peripherals().await?;
        for peripheral in peripherals.iter() {
            let properties = peripheral.properties().await?;
            if let Some(props) = properties {
                if props.services.contains(&SERVICE_UUID) {
                    self.found_peripherals.push(peripheral.clone());
                    discovered_devices.push(Device {
                        name: props.local_name.unwrap_or_default(),
                        address: props.address.to_string(),
                        id: "".to_string(),
                        signal_strength: props.rssi.unwrap_or(-100).into(),
                        state: DeviceState::Disconnected,
                    });
                }
            }
        }
        Ok(discovered_devices)
    }

    pub async fn ble_connect_toothpaste(&mut self, device: Device) -> Result<String, Box<dyn std::error::Error>> {
        let mut peripheral = None;
        for p in self.found_peripherals.iter() {
            if let Ok(Some(props)) = p.properties().await {
                if props.address == device.address.parse()? {
                    peripheral = Some(p.clone());
                    break;
                }
            }
        }
        let peripheral = peripheral.ok_or("Peripheral not found")?;

        if !peripheral.is_connected().await? {
            match peripheral.connect().await {
                Ok(_) => self.connected_peripherial = Some(peripheral.clone()),
                Err(err) => {
                    eprintln!("Failed to connect to peripheral: {}, Error: {}", device.name, err);
                    return Err("Connection failed".into());
                }
            };
        };

        time::sleep(Duration::from_millis(200)).await;
        self.connected_peripherial.as_ref().unwrap().discover_services().await?;

        // Get all characteristics
        let service = self.get_service_with_retry(&peripheral, SERVICE_UUID).await?;
        let packet_char = self.get_characteristic_with_retry(&service, PACKET_CHARACTERISTIC_UUID).await?;
        let semaphore_char = self.get_characteristic_with_retry(&service, HID_SEMAPHORE_CHARACTERISTIC_UUID).await?;
        let mac_char = self.get_characteristic_with_retry(&service, MAC_ADDRESS_CHARACTERISTIC_UUID).await?;

        let mac_data = self.connected_peripherial.as_ref().unwrap().read(&mac_char).await?;
        let mac_str = mac_data.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        println!("Connected to device, MAC: {}", mac_str);

        self.cached_peripheral = Some(CachedPeripheral { packet_char, semaphore_char, mac_char });
        Ok(mac_str)
    }

    async fn get_service_with_retry(&self, peripheral: &Peripheral, uuid: Uuid) -> Result<btleplug::api::Service, Box<dyn std::error::Error>> {
        let attempts = 3;
        for attempt in 0..attempts {
            match peripheral.services().iter().find(|s| s.uuid == uuid).cloned() {
                Some(service) => return Ok(service),
                None => {
                    if attempt < attempts - 1 {
                        eprintln!("Retrying service discovery... (attempt {})", attempt + 1);
                        time::sleep(Duration::from_millis(300)).await;
                    } else {
                        return Err("Service not found".into());
                    }
                }
            }
        }
        Err("Service not found after retries".into())
    }

    async fn get_characteristic_with_retry(&self, service: &btleplug::api::Service, uuid: Uuid) -> Result<btleplug::api::Characteristic, Box<dyn std::error::Error>> {
        let attempts = 3;
        for attempt in 0..attempts {
            match service.characteristics.iter().find(|c| c.uuid == uuid).cloned() {
                Some(characteristic) => return Ok(characteristic),
                None => {
                    if attempt < attempts - 1 {
                        eprintln!("Retrying characteristic discovery... (attempt {})", attempt + 1);
                        time::sleep(Duration::from_millis(300)).await;
                    } else {
                        return Err("Characteristic not found".into());
                    }
                }
            }
        }
        Err("Characteristic not found after retries".into())
    }

    pub async fn ble_send_unencrypted(&self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        let packet = toothpaste_desktop_proto::packets::create_unencrypted_packet(data);
        let packet_char = &self.cached_peripheral.as_ref().ok_or("No cached peripheral")?.packet_char;
        self.connected_peripherial.as_ref().unwrap()
            .write(packet_char, &packet, btleplug::api::WriteType::WithoutResponse).await?;
        Ok(())
    }

    pub async fn ble_send_unencrypted_packet(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let packet_char = &self.cached_peripheral.as_ref().ok_or("No cached peripheral")?.packet_char;
        self.connected_peripherial.as_ref().unwrap()
            .write(packet_char, data, btleplug::api::WriteType::WithoutResponse).await?;
        Ok(())
    }

    pub async fn ble_send_encrypted(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let packet_char = &self.cached_peripheral.as_ref().ok_or("No cached peripheral")?.packet_char;
        self.connected_peripherial.as_ref().unwrap()
            .write(packet_char, data, btleplug::api::WriteType::WithoutResponse).await?;
        Ok(())
    }

    pub async fn subscribe_notifications(
        &self,
    ) -> Result<futures::stream::BoxStream<'static, btleplug::api::ValueNotification>, Box<dyn std::error::Error>> {
        let peripheral = self.connected_peripherial.as_ref().ok_or("Not connected")?;
        let cached = self.cached_peripheral.as_ref().ok_or("No cached peripheral")?;
        let semchar = &cached.semaphore_char;

        peripheral.subscribe(semchar).await?;
        let stream = peripheral.notifications().await?;
        
        Ok(Box::pin(stream))
    }
}

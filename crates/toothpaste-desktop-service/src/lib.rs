use std::time::Duration;
use tokio::time;
use uuid::{uuid, Uuid};

use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use toothpaste_desktop_proto::toothpaste::response_packet::ResponseType;
use toothpaste_desktop_proto::toothpaste::response_packet;

pub mod storage;
pub mod crypto;

/// Implement this trait to handle incoming `ResponsePacket` notifications from the device.
/// Return `Some(bytes)` to write a packet back over BLE; `None` to send nothing.
pub trait ResponseHandler {
    async fn on_keepalive(&mut self) -> Option<Vec<u8>>;
    async fn on_peer_unknown(&mut self) -> Option<Vec<u8>>;
    async fn on_peer_known(&mut self, firmware_version: &str) -> Option<Vec<u8>>;
    async fn on_challenge(&mut self, challenge_data: &[u8]) -> Option<Vec<u8>>;
}

const SERVICE_UUID: Uuid                    = uuid!("19b10000-e8f2-537e-4f6c-d104768a1214");
const PACKET_CHARACTERISTIC_UUID: Uuid      = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5907");
const HID_SEMAPHORE_CHARACTERISTIC_UUID: Uuid = uuid!("6856e119-2c7b-455a-bf42-cf7ddd2c5908");
const MAC_ADDRESS_CHARACTERISTIC_UUID: Uuid = uuid!("19b10002-e8f2-537e-4f6c-d104768a1214");

// Struct to cache characteristics after connecting to a peripheral
pub struct CachedPeripheral{
    packet_char: Characteristic,
    semaphore_char: Characteristic,
    mac_char: Characteristic,
}

// The main BLE structure to manage Bluetooth operations
pub struct BleManager {
    manager : Manager,
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
            connected_peripherial: None, // Placeholder, will be set on connect
            cached_peripheral: None,
        })
    }
    pub async fn ble_discover_toothpaste(&mut self) -> Result<Vec<String>, btleplug::Error> {

        let mut discovered_devices: Vec<String> = Vec::new();
        // Directly filtering by service UUID doesn't work reliably
        self.adapter.start_scan(ScanFilter::default()).await.expect("Failed to start scan");
        time::sleep(Duration::from_secs(5)).await;

        let peripherals = self.adapter.peripherals().await?;
        for peripheral in peripherals.iter() {
            let properties = peripheral.properties().await?;
            if let Some(props) = properties {
                if props.services.contains(&SERVICE_UUID) {
                    self.found_peripherals.push(peripheral.clone());
                    discovered_devices.push(props.local_name.unwrap_or_default());
                }
            }
        }
        Ok(discovered_devices)
    }

    pub async fn ble_connect_toothpaste(&mut self, peripheral_name: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Connect to the device - find peripheral by name
        let mut peripheral = None;
        for p in self.found_peripherals.iter() {
            if let Ok(Some(props)) = p.properties().await {
                if props.local_name == Some(peripheral_name.into()) {
                    peripheral = Some(p.clone());
                    break;
                }
            }
        }
        let peripheral = peripheral.ok_or("Peripheral not found")?;

        // Attempt to connect if not already connected
        if !peripheral.is_connected().await? {
            match peripheral.connect().await {
                Ok(_) => self.connected_peripherial = Some(peripheral.clone()),
                Err(err) => {
                    eprintln!("Failed to connect to peripheral: {}, Error: {}", peripheral_name, err);
                    return Err("Connection failed".into());
                }
            };
        };

        // Wait a bit before getting GATT information
        time::sleep(Duration::from_millis(200)).await;

        // Discover services
        self.connected_peripherial.as_ref().unwrap().discover_services().await?;

        // Get the service with retry logic
        let service = self.get_service_with_retry(&peripheral, SERVICE_UUID).await?;

        // Get characteristics with retry logic
        let packet_char = self.get_characteristic_with_retry(&service, PACKET_CHARACTERISTIC_UUID).await?;
        let semaphore_char = self.get_characteristic_with_retry(&service, HID_SEMAPHORE_CHARACTERISTIC_UUID).await?;
        let mac_char = self.get_characteristic_with_retry(&service, MAC_ADDRESS_CHARACTERISTIC_UUID).await?;

        // Read MAC address from the device
        let mac_data = self.connected_peripherial.as_ref().unwrap().read(&mac_char).await?;
        let mac_str = mac_data
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        println!("Connected to device: MAC: {}", mac_str);

        // Cache the characteristics for later use
        self.cached_peripheral = Some(CachedPeripheral {
            packet_char,
            semaphore_char,
            mac_char,
        });
        
        Ok(mac_str)
    }

    // TODO: Genericise over service and characteristic 
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

    // TODO: Genericise over service and characteristic 
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
        let unencrypted_packet = toothpaste_desktop_proto::packets::create_unencrypted_packet(data);
        let packet_char: &Characteristic = &self.cached_peripheral.as_ref().ok_or("No cached peripheral")?.packet_char;

        self.connected_peripherial.as_ref().unwrap()
            .write(&packet_char, &unencrypted_packet, btleplug::api::WriteType::WithoutResponse).await?;
        
        Ok(())
    }

    pub async fn subscribe_notifications<H: ResponseHandler>(
        &self,
        handler: &mut H,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let peripheral = self.connected_peripherial.as_ref().ok_or("Not connected")?;
        let cached = self.cached_peripheral.as_ref().ok_or("No cached peripheral")?;
        let semchar = &cached.semaphore_char;
        let packet_char = &cached.packet_char;

        peripheral.subscribe(semchar).await?;
        let mut stream = peripheral.notifications().await?;

        while let Some(notification) = stream.next().await {
            if notification.uuid != HID_SEMAPHORE_CHARACTERISTIC_UUID {
                continue;
            }

            let packet = match toothpaste_desktop_proto::packets::unpack_response_packet(&notification.value) {
                Ok(p) => p,
                Err(e) => { eprintln!("Failed to decode ResponsePacket: {e}"); continue; }
            };

            let response = match response_packet::ResponseType::try_from(packet.response_type) {
                Ok(ResponseType::Keepalive)   => handler.on_keepalive().await,
                Ok(ResponseType::PeerUnknown) => handler.on_peer_unknown().await,
                Ok(ResponseType::PeerKnown)   => handler.on_peer_known(&packet.firmware_version).await,
                Ok(ResponseType::Challenge)   => handler.on_challenge(&packet.challenge_data).await,
                Err(_) => { eprintln!("Unknown response type: {}", packet.response_type); None }
            };

            if let Some(bytes) = response {
                if let Err(e) = peripheral.write(packet_char, &bytes, btleplug::api::WriteType::WithoutResponse).await {
                    eprintln!("Failed to send response packet: {e}");
                }
            }
        }

        Ok(())
    }

}

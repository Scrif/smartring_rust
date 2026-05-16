use std::fs::File;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use btleplug::api::{Central, Characteristic, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Peripheral};
use futures::StreamExt;
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

use crate::packet::{Packet, PacketError};

/// NordicSemiconductor UART Service used by Colmi rings.
const RX_UUID: Uuid = Uuid::from_u128(0x6e400002_b5a3_f393_e0a9_e50e24dcca9e);
const TX_UUID: Uuid = Uuid::from_u128(0x6e400003_b5a3_f393_e0a9_e50e24dcca9e);

/// How long to scan before looking up a peripheral by address or name.
const SCAN_DURATION: Duration = Duration::from_secs(5);

/// Per-reply receive timeout.
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("RX characteristic not found — is this a Colmi ring?")]
    RxNotFound,
    #[error("TX characteristic not found — is this a Colmi ring?")]
    TxNotFound,
    #[error("timed out waiting for reply packet")]
    Timeout,
    #[error("unexpected packet length: expected 16 bytes, got {0}")]
    InvalidPacketLength(usize),
    #[error("BLE error: {0}")]
    Btleplug(#[from] btleplug::Error),
    #[error("packet error: {0}")]
    Packet(#[from] PacketError),
}

/// A connected BLE client for a Colmi ring.
pub struct Client {
    peripheral: Peripheral,
    /// The write (client→ring) characteristic.
    rx: Characteristic,
    /// Optional binary capture file. Every received packet is appended as raw bytes.
    record: Option<Mutex<File>>,
}

impl Client {
    /// Connect to a ring by its Bluetooth address (preferred on Linux/Windows).
    pub async fn connect(adapter: &Adapter, address: &str) -> Result<Self, ClientError> {
        debug!("scanning to locate device with address {}", address);
        adapter.start_scan(ScanFilter::default()).await?;
        tokio::time::sleep(SCAN_DURATION).await;
        adapter.stop_scan().await?;

        let peripherals = adapter.peripherals().await?;
        for p in peripherals {
            // Compare against p.id() — on macOS this is the per-session UUID that
            // btleplug::api::PeripheralProperties::address cannot surface (it returns
            // an all-zero BDAddr because Core Bluetooth withholds hardware MACs).
            if p.id().to_string().eq_ignore_ascii_case(address) {
                return Self::from_peripheral(p).await;
            }
        }
        Err(ClientError::DeviceNotFound(address.to_string()))
    }

    /// Connect to a ring by device name (reliable on macOS where addresses are session UUIDs).
    pub async fn connect_by_name(adapter: &Adapter, name: &str) -> Result<Self, ClientError> {
        debug!("scanning to locate device with name {}", name);
        adapter.start_scan(ScanFilter::default()).await?;
        tokio::time::sleep(SCAN_DURATION).await;
        adapter.stop_scan().await?;

        let peripherals = adapter.peripherals().await?;
        for p in peripherals {
            if let Ok(Some(props)) = p.properties().await {
                if props.local_name.as_deref() == Some(name) {
                    return Self::from_peripheral(p).await;
                }
            }
        }
        Err(ClientError::DeviceNotFound(name.to_string()))
    }

    /// Enable binary packet capture: every received packet is appended to `path`.
    ///
    /// Creates `path` and any missing parent directories.
    /// Call this after [`connect`] / [`connect_by_name`] and before the first command.
    pub fn with_recording(mut self, path: PathBuf) -> Result<Self, std::io::Error> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        self.record = Some(Mutex::new(File::create(path)?));
        Ok(self)
    }

    async fn from_peripheral(peripheral: Peripheral) -> Result<Self, ClientError> {
        peripheral.connect().await?;
        peripheral.discover_services().await?;

        let chars = peripheral.characteristics();

        let rx = chars
            .iter()
            .find(|c| c.uuid == RX_UUID)
            .ok_or(ClientError::RxNotFound)?
            .clone();

        let tx = chars
            .iter()
            .find(|c| c.uuid == TX_UUID)
            .ok_or(ClientError::TxNotFound)?
            .clone();

        peripheral.subscribe(&tx).await?;
        debug!("connected and subscribed to TX notifications");

        Ok(Client { peripheral, rx, record: None })
    }

    /// Write `packet` to the ring and collect `expected_replies` notification packets.
    ///
    /// Pass `expected_replies = 0` for fire-and-forget commands (e.g. reboot, set-time).
    pub async fn send_recv(
        &self,
        packet: Packet,
        expected_replies: usize,
    ) -> Result<Vec<Packet>, ClientError> {
        debug!("→ TX cmd={:#04x} bytes={:02x?}", packet.command, packet.as_bytes());

        if expected_replies == 0 {
            self.peripheral
                .write(&self.rx, &packet.as_bytes(), WriteType::WithoutResponse)
                .await?;
            return Ok(vec![]);
        }

        // Open the notification stream BEFORE writing so we cannot miss a fast reply.
        let mut stream = self.peripheral.notifications().await?;

        self.peripheral
            .write(&self.rx, &packet.as_bytes(), WriteType::WithoutResponse)
            .await?;

        let mut replies = Vec::with_capacity(expected_replies);
        while replies.len() < expected_replies {
            match tokio::time::timeout(RECV_TIMEOUT, stream.next()).await {
                Ok(Some(notif)) if notif.uuid == TX_UUID => {
                    let len = notif.value.len();
                    let raw: [u8; 16] = notif.value.try_into().map_err(|_| {
                        ClientError::InvalidPacketLength(len)
                    })?;
                    debug!("← RX cmd={:#04x} bytes={:02x?}", raw[0], raw);
                    self.record_packet(&raw);
                    replies.push(Packet::from_bytes(raw)?);
                }
                Ok(Some(_)) => {
                    // Notification from a different characteristic; skip.
                }
                Ok(None) => break,
                Err(_) => return Err(ClientError::Timeout),
            }
        }

        Ok(replies)
    }

    /// Write `packet` and collect replies until `is_done(&replies)` returns true.
    ///
    /// Stops early when `is_done` signals completion, the `max_packets` cap is hit,
    /// the notification stream closes, or the per-packet [`RECV_TIMEOUT`] fires.
    ///
    /// Use this for commands with variable-length responses (e.g. sport-detail,
    /// real-time readings) where the total packet count is not known upfront.
    pub async fn send_recv_until<F>(
        &self,
        packet: Packet,
        max_packets: usize,
        is_done: F,
    ) -> Result<Vec<Packet>, ClientError>
    where
        F: Fn(&[Packet]) -> bool,
    {
        debug!("→ TX cmd={:#04x} bytes={:02x?}", packet.command, packet.as_bytes());

        let mut stream = self.peripheral.notifications().await?;

        self.peripheral
            .write(&self.rx, &packet.as_bytes(), WriteType::WithoutResponse)
            .await?;

        let mut replies = Vec::new();
        loop {
            if replies.len() >= max_packets {
                break;
            }
            match tokio::time::timeout(RECV_TIMEOUT, stream.next()).await {
                Ok(Some(notif)) if notif.uuid == TX_UUID => {
                    let len = notif.value.len();
                    let raw: [u8; 16] = notif.value.try_into().map_err(|_| {
                        ClientError::InvalidPacketLength(len)
                    })?;
                    debug!("← RX cmd={:#04x} bytes={:02x?}", raw[0], raw);
                    self.record_packet(&raw);
                    replies.push(Packet::from_bytes(raw)?);
                    if is_done(&replies) {
                        break;
                    }
                }
                Ok(Some(_)) => {
                    // Notification from a different characteristic; skip.
                }
                Ok(None) => break,
                Err(_) => return Err(ClientError::Timeout),
            }
        }

        Ok(replies)
    }

    /// Append raw packet bytes to the capture file, if recording is active.
    ///
    /// Errors are silently swallowed — a capture write failure must not abort a command.
    fn record_packet(&self, raw: &[u8; 16]) {
        if let Some(ref mutex) = self.record {
            if let Ok(mut file) = mutex.lock() {
                let _ = file.write_all(raw);
            }
        }
    }

    /// Returns the platform-specific identifier for the connected peripheral.
    ///
    /// On macOS this is the Core Bluetooth UUID (e.g. `"6F4066B7-C831-D78E-396C-DE27CE3FBF4E"`).
    /// On Linux/Windows it is the hardware MAC address (e.g. `"AA:BB:CC:DD:EE:FF"`).
    pub fn peripheral_address(&self) -> String {
        self.peripheral.id().to_string()
    }

    /// Return the path this client is recording to, if recording is active.
    pub fn recording_path(&self) -> Option<&Path> {
        // Not stored separately; used only for testing via with_recording's existence.
        None
    }
}

#[cfg(test)]
mod tests {
    // Integration tests require a physical ring and are excluded from CI.
    // Run manually with: cargo test -p smartring-core client -- --ignored

    #[tokio::test]
    #[ignore = "requires a connected Colmi ring"]
    async fn connect_by_address_round_trip() {
        // Placeholder — fill in a real address before running.
    }

    #[tokio::test]
    #[ignore = "requires a connected Colmi ring"]
    async fn connect_by_name_round_trip() {
        // Placeholder — fill in a real device name before running.
    }
}

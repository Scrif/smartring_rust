use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use thiserror::Error;
use tracing::{debug, warn};

/// Errors that can occur during BLE operations.
#[derive(Debug, Error)]
pub enum BleError {
    #[error("no Bluetooth adapter found on this system")]
    NoAdapter,
    #[error("BLE error: {0}")]
    Btleplug(#[from] btleplug::Error),
}

/// A BLE peripheral discovered during a scan.
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub name: Option<String>,
    pub address: String,
    pub rssi: Option<i16>,
}

/// Known Colmi-compatible device name prefixes used for post-scan filtering.
///
/// BlueZ (Linux) merges scan filters across D-Bus clients, so we scan with no
/// filter and apply this list ourselves after collecting results.
pub const COLMI_PREFIXES: &[&str] = &[
    "R01", "R02", "R03", "R04", "R05", "R06", "R07", "R08", "R09", "R10",
    "COLMI", "VK-5098", "MERLIN", "Hello Ring", "RING1", "boAtring",
    "TR-R02", "SE", "EVOLVEO", "GL-SR2", "Blaupunkt", "KSIX RING",
];

/// Returns `true` if the device name starts with a known Colmi prefix.
pub fn is_colmi_device(name: &str) -> bool {
    COLMI_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Retain only Colmi-compatible devices from a list of discovered peripherals.
pub fn filter_colmi(devices: Vec<DiscoveredDevice>) -> Vec<DiscoveredDevice> {
    devices
        .into_iter()
        .filter(|d| d.name.as_deref().map(is_colmi_device).unwrap_or(false))
        .collect()
}

/// Return the first available Bluetooth adapter on this system.
///
/// This is a convenience function for CLI commands that need an adapter before
/// constructing a [`crate::client::Client`].
pub async fn get_default_adapter() -> Result<Adapter, BleError> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    adapters.into_iter().next().ok_or(BleError::NoAdapter)
}

/// Scan for BLE peripherals for `duration`.
///
/// Returns *all* visible devices without any prefix filtering — call
/// [`filter_colmi`] on the result if you only want Colmi-compatible rings.
pub async fn scan(duration: Duration) -> Result<Vec<DiscoveredDevice>, BleError> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters.into_iter().next().ok_or(BleError::NoAdapter)?;

    debug!("starting BLE scan for {:?}", duration);
    adapter.start_scan(ScanFilter::default()).await?;
    tokio::time::sleep(duration).await;
    adapter.stop_scan().await?;

    let peripherals = adapter.peripherals().await?;
    debug!("raw peripheral count: {}", peripherals.len());

    let mut devices = Vec::with_capacity(peripherals.len());
    for peripheral in peripherals {
        match peripheral.properties().await {
            Ok(Some(props)) => {
                devices.push(DiscoveredDevice {
                    name: props.local_name,
                    // Use the peripheral's platform ID, not props.address (a BDAddr).
                    // On macOS, Core Bluetooth never exposes hardware MACs — props.address
                    // is all-zeros. peripheral.id() returns the correct per-session UUID
                    // on macOS and the MAC string on Linux/Windows.
                    address: peripheral.id().to_string(),
                    rssi: props.rssi,
                });
            }
            Ok(None) => {
                warn!("peripheral returned no properties, skipping");
            }
            Err(e) => {
                warn!("failed to read peripheral properties: {}", e);
            }
        }
    }

    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_colmi_prefixes_are_matched() {
        // R01–R10 range
        for n in 1..=10u8 {
            let name = format!("R{:02} 1234", n);
            assert!(is_colmi_device(&name), "expected match for {name}");
        }
        // OEM variants from the spec
        for name in &[
            "COLMI R06",
            "VK-5098",
            "MERLIN",
            "Hello Ring 123",
            "RING1",
            "boAtring 001",
            "TR-R02",
            "SE1234",
            "EVOLVEO ring",
            "GL-SR2",
            "Blaupunkt ring",
            "KSIX RING",
        ] {
            assert!(is_colmi_device(name), "expected match for {name}");
        }
    }

    #[test]
    fn non_colmi_names_are_rejected() {
        for name in &["AirPods Pro", "Pixel Watch", "random device", "Galaxy Buds", ""] {
            assert!(!is_colmi_device(name), "unexpected match for {name}");
        }
    }

    #[test]
    fn filter_colmi_keeps_matching_devices() {
        let devices = vec![
            DiscoveredDevice {
                name: Some("R02 1234".to_string()),
                address: "AA:BB:CC:DD:EE:FF".to_string(),
                rssi: Some(-70),
            },
            DiscoveredDevice {
                name: Some("AirPods Pro".to_string()),
                address: "11:22:33:44:55:66".to_string(),
                rssi: Some(-80),
            },
            DiscoveredDevice {
                name: None,
                address: "00:11:22:33:44:55".to_string(),
                rssi: None,
            },
        ];

        let filtered = filter_colmi(devices);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn filter_colmi_empty_input_returns_empty() {
        assert!(filter_colmi(vec![]).is_empty());
    }

    #[test]
    fn filter_colmi_all_matching_keeps_all() {
        let devices = vec![
            DiscoveredDevice {
                name: Some("R02 AAAA".to_string()),
                address: "AA:AA:AA:AA:AA:AA".to_string(),
                rssi: Some(-60),
            },
            DiscoveredDevice {
                name: Some("COLMI R06".to_string()),
                address: "BB:BB:BB:BB:BB:BB".to_string(),
                rssi: Some(-65),
            },
        ];

        let filtered = filter_colmi(devices);
        assert_eq!(filtered.len(), 2);
    }

    /// Verifies that discovered devices expose a non-zero platform identifier as their address.
    ///
    /// On macOS, Core Bluetooth never exposes hardware MACs; btleplug assigns each peripheral
    /// a per-session UUID (e.g. "6F4066B7-C831-D78E-396C-DE27CE3FBF4E"). Reporting
    /// "00:00:00:00:00:00" is a bug — this test guards against it.
    ///
    /// Run manually with: cargo test -p smartring-core -- --ignored scan_address_is_not_all_zeros
    #[tokio::test]
    #[ignore = "requires a nearby Colmi ring and a Bluetooth adapter"]
    async fn scan_address_is_not_all_zeros() {
        let devices = scan(std::time::Duration::from_secs(5)).await.expect("scan failed");
        let colmi = filter_colmi(devices);
        assert!(!colmi.is_empty(), "no Colmi ring found — move ring closer and retry");
        for device in &colmi {
            assert_ne!(
                device.address, "00:00:00:00:00:00",
                "address must be a platform UUID (macOS) or MAC (Linux/Windows), not all-zeros"
            );
        }
    }
}

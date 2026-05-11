use std::time::Duration;

use anyhow::Result;
use clap::Args;
use smartring_core::ble;

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Scan duration in seconds
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,

    /// Show all BLE devices, not just Colmi-compatible rings
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

pub async fn run(args: ScanArgs) -> Result<()> {
    let duration = Duration::from_secs(args.timeout);
    let devices = ble::scan(duration).await?;

    let devices = if args.all {
        devices
    } else {
        ble::filter_colmi(devices)
    };

    if devices.is_empty() {
        eprintln!("No devices found — try moving the ring closer");
        return Ok(());
    }

    println!("{:<32} {}", "Name", "Address");
    println!("{}", "-".repeat(52));
    for device in &devices {
        let name = device.name.as_deref().unwrap_or("(unknown)");
        println!("{:<32} {}", name, device.address);
    }

    Ok(())
}

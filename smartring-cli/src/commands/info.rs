use anyhow::Result;
use smartring_core::client::Client;
use smartring_core::device_info::{battery_request, device_info_request, parse_battery, parse_device_info};

pub async fn run(client: &Client) -> Result<()> {
    let battery_replies = client.send_recv(battery_request(), 1).await?;
    let battery = parse_battery(&battery_replies[0])?;

    let info_replies = client.send_recv(device_info_request(), 1).await?;
    let info = parse_device_info(&info_replies[0])?;

    println!("Firmware: {}", info.firmware_version);
    println!("Hardware: {}", info.hardware_version);
    println!("Battery:  {}%", battery.0);

    Ok(())
}

use anyhow::{bail, Context, Result};
use clap::Args;

use smartring_core::client::Client;
use smartring_core::hr_settings::{
    get_hr_settings_packet, parse_hr_settings, set_hr_settings_packet, HrSettings,
};

// ── get-heart-rate-log-settings ───────────────────────────────────────────────

pub async fn run_get(client: &Client) -> Result<()> {
    let packets = client
        .send_recv(get_hr_settings_packet(), 1)
        .await
        .context("failed to fetch HR log settings from ring")?;

    let pkt = packets.into_iter().next().context("no response from ring")?;
    let settings = parse_hr_settings(&pkt).context("failed to parse HR log settings response")?;

    println!("Enabled:  {}", settings.enabled);
    println!("Interval: {} minutes", settings.interval);
    Ok(())
}

// ── set-heart-rate-log-settings ───────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct SetHrSettingsArgs {
    /// Enable automatic heart rate logging
    #[arg(long, conflicts_with = "disable")]
    pub enable: bool,

    /// Disable automatic heart rate logging
    #[arg(long, conflicts_with = "enable")]
    pub disable: bool,

    /// Logging interval in minutes (1–255)
    #[arg(long, value_name = "MINUTES", value_parser = clap::value_parser!(u8).range(1..))]
    pub interval: u8,
}

pub async fn run_set(args: &SetHrSettingsArgs, client: &Client) -> Result<()> {
    if !args.enable && !args.disable {
        bail!("pass either --enable or --disable");
    }

    let settings = HrSettings {
        enabled: args.enable,
        interval: args.interval,
    };

    client
        .send_recv(set_hr_settings_packet(&settings), 1)
        .await
        .context("failed to apply HR log settings to ring")?;

    println!("Enabled:  {}", settings.enabled);
    println!("Interval: {} minutes", settings.interval);
    Ok(())
}

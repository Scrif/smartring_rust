use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Args;
use smartring_core::client::Client;
use smartring_core::set_time::set_time_packet;

#[derive(Debug, Args)]
pub struct SetTimeArgs {
    /// UTC datetime to set (ISO 8601, e.g. "2026-05-11T12:00:00Z").
    /// Defaults to the current system time.
    #[arg(long, value_name = "DATETIME")]
    pub when: Option<String>,
}

pub async fn run(args: &SetTimeArgs, client: &Client) -> Result<()> {
    let dt: DateTime<Utc> = match args.when.as_deref() {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .context("--when must be an ISO 8601 datetime, e.g. 2026-05-11T12:00:00Z")?
            .into(),
        None => Utc::now(),
    };

    client.send_recv(set_time_packet(dt), 0).await?;
    println!("Ring time set to {}", dt.format("%Y-%m-%d %H:%M:%S UTC"));
    Ok(())
}

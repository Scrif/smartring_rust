use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Args;
use smartring_core::client::Client;
use smartring_core::hr::{heart_rate_log_request, parse_heart_rate_log, HR_LOG_PACKETS};

#[derive(Debug, Args)]
pub struct GetHeartRateLogArgs {
    /// Date to fetch (YYYY-MM-DD). Defaults to today (UTC).
    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,
}

pub async fn run(args: &GetHeartRateLogArgs, client: &Client) -> Result<()> {
    let date = match args.date.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .context("--date must be in YYYY-MM-DD format, e.g. 2026-05-11")?,
        None => chrono::Utc::now().date_naive(),
    };

    let request = heart_rate_log_request(date);
    let packets = client.send_recv(request, HR_LOG_PACKETS).await?;
    let log = parse_heart_rate_log(date, &packets)?;

    let pairs = log.readings_with_times();
    if pairs.is_empty() {
        println!("No heart rate readings for {date}.");
    } else {
        for (ts, bpm) in pairs {
            println!("{}  {} bpm", ts.format("%Y-%m-%d %H:%M:%S UTC"), bpm);
        }
    }

    Ok(())
}

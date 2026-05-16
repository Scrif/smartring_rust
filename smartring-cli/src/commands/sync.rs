use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use clap::Args;
use tracing::debug;

use smartring_core::client::{Client, ClientError};
use smartring_core::db::Db;
use smartring_core::hr::{heart_rate_log_request, parse_heart_rate_log, HR_LOG_PACKETS};
use smartring_core::set_time::set_time_packet;

const TOOL_VERSION: &str = concat!("smartring-manager/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// SQLite database file to write into.
    #[arg(long, value_name = "PATH", default_value = "ring_data.sqlite")]
    pub db: PathBuf,

    /// First day to sync, inclusive (YYYY-MM-DD).
    /// Defaults to the day after the last sync, or 7 days ago if never synced.
    #[arg(long, value_name = "DATE")]
    pub start: Option<String>,

    /// Last day to sync, inclusive (YYYY-MM-DD). Defaults to today (UTC).
    #[arg(long, value_name = "DATE")]
    pub end: Option<String>,
}

pub async fn run(args: &SyncArgs, client: &Client, address: &str, name: Option<&str>) -> Result<()> {
    let db = Db::open(&args.db)
        .with_context(|| format!("failed to open database at {}", args.db.display()))?;

    let ring_id = db
        .create_or_find_ring(address, name)
        .context("failed to upsert ring row")?;

    let end_date = match args.end.as_deref() {
        Some(s) => parse_date(s).context("--end must be in YYYY-MM-DD format, e.g. 2026-05-10")?,
        None => Utc::now().date_naive(),
    };

    let start_date = match args.start.as_deref() {
        Some(s) => {
            parse_date(s).context("--start must be in YYYY-MM-DD format, e.g. 2026-05-01")?
        }
        None => match db
            .get_last_sync_time(ring_id)
            .context("failed to query last sync time")?
        {
            // Resume from the day after the last sync so we don't re-fetch already-stored days.
            Some(last) => last.date_naive() + Duration::days(1),
            None => Utc::now().date_naive() - Duration::days(7),
        },
    };

    println!("Syncing from {} to {}", start_date, end_date);
    println!("Writing to {}", args.db.display());

    debug!("steps sync is not yet implemented; steps table will remain empty");

    let sync_id = db
        .create_sync(ring_id, TOOL_VERSION)
        .context("failed to create sync record")?;

    let mut date = start_date;
    while date <= end_date {
        // A timeout on the heart rate fetch means the ring sent fewer than the
        // expected 12 packets — this happens on days with no recorded data.
        // Treat it as empty for this day and continue rather than aborting.
        let packets = match client
            .send_recv(heart_rate_log_request(date), HR_LOG_PACKETS)
            .await
        {
            Ok(pkts) => pkts,
            Err(ClientError::Timeout) => {
                debug!("heart rate log for {date} incomplete — no data recorded");
                vec![]
            }
            Err(e) => {
                return Err(e).with_context(|| format!("failed to fetch heart rate log for {date}"))
            }
        };

        let log = parse_heart_rate_log(date, &packets)
            .with_context(|| format!("failed to parse heart rate log for {date}"))?;

        let readings = log.readings_with_times();
        if !readings.is_empty() {
            db.insert_heart_rates(ring_id, sync_id, &readings)
                .with_context(|| format!("failed to write heart rate readings for {date}"))?;
        }

        // Brief pause between consecutive requests so the ring firmware has time
        // to reset its internal state before handling the next command.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        date += Duration::days(1);
    }

    // Set ring time after sync, matching Python tool behaviour.
    client
        .send_recv(set_time_packet(Utc::now()), 0)
        .await
        .context("failed to set ring time after sync")?;

    println!("Done");
    Ok(())
}

fn parse_date(s: &str) -> Result<NaiveDate, chrono::ParseError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
}

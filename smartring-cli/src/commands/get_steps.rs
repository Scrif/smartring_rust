use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Args;
use smartring_core::client::Client;
use smartring_core::steps::{
    parse_sport_details, sport_detail_done, sport_detail_request, StepsResult, MAX_SPORT_PACKETS,
};

use crate::output::{print_steps_csv, print_steps_table};

#[derive(Debug, Args)]
pub struct GetStepsArgs {
    /// Date to fetch (YYYY-MM-DD). Defaults to today (UTC).
    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,

    /// Output as a JSON array.
    #[arg(long)]
    pub json: bool,

    /// Output as RFC 4180 CSV with a header row.
    #[arg(long)]
    pub csv: bool,
}

pub async fn run(args: &GetStepsArgs, client: &Client) -> Result<()> {
    let today = chrono::Utc::now().date_naive();

    let date = match args.date.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .context("--date must be in YYYY-MM-DD format, e.g. 2026-05-11")?,
        None => today,
    };

    let offset = (today - date).num_days();
    if offset < 0 {
        anyhow::bail!("--date cannot be in the future (got {})", date);
    }
    if offset > 6 {
        eprintln!(
            "Warning: requesting data more than 7 days old (offset {offset}); \
             the ring may not have this data."
        );
    }

    let request = sport_detail_request(offset as u8);
    let packets = client
        .send_recv_until(request, MAX_SPORT_PACKETS, sport_detail_done)
        .await?;
    let result = parse_sport_details(&packets)?;

    match result {
        StepsResult::NoData => {
            println!("No data for day {date}.");
        }
        StepsResult::Data(details) => {
            if args.json {
                let json = serde_json::to_string_pretty(&details)
                    .context("failed to serialize steps to JSON")?;
                println!("{json}");
            } else if args.csv {
                print_steps_csv(&details);
            } else {
                print_steps_table(&details);
            }
        }
    }

    Ok(())
}

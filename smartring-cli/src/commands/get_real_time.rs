use std::time::Duration;

use anyhow::Result;
use clap::{Args, ValueEnum};
use smartring_core::client::Client;
use smartring_core::real_time;

/// Which real-time sensor reading to request.
#[derive(Debug, Clone, ValueEnum)]
pub enum ReadingType {
    /// Heart rate in beats per minute
    #[value(name = "heart-rate")]
    HeartRate,
    /// Blood oxygen saturation percentage
    #[value(name = "spo2")]
    Spo2,
}

#[derive(Debug, Args)]
pub struct GetRealTimeArgs {
    /// Type of reading to take
    pub reading_type: ReadingType,
}

pub async fn run(args: &GetRealTimeArgs, client: &Client) -> Result<()> {
    let (start_pkt, stop_pkt, timeout, type_label) = match args.reading_type {
        ReadingType::HeartRate => (
            real_time::real_time_hr_start(),
            real_time::real_time_hr_stop(),
            Duration::from_secs(30),
            "heart rate",
        ),
        ReadingType::Spo2 => (
            real_time::real_time_spo2_start(),
            real_time::real_time_spo2_stop(),
            Duration::from_secs(60),
            "SpO2",
        ),
    };

    let result = tokio::time::timeout(
        timeout,
        client.send_recv_until(
            start_pkt,
            real_time::MAX_REAL_TIME_PACKETS,
            |pkts| match args.reading_type {
                ReadingType::HeartRate => real_time::heart_rate_done(pkts),
                ReadingType::Spo2 => real_time::spo2_done(pkts),
            },
        ),
    )
    .await;

    // Always ask the ring to stop the sensor, regardless of outcome.
    // Fire-and-forget: ignore errors here since the main result is what matters.
    let _ = client.send_recv(stop_pkt, 0).await;

    match result {
        Ok(Ok(packets)) => {
            let reading = match args.reading_type {
                ReadingType::HeartRate => {
                    packets.iter().find_map(|p| real_time::extract_heart_rate(p))
                }
                ReadingType::Spo2 => {
                    packets.iter().find_map(|p| real_time::extract_spo2(p))
                }
            };
            match reading {
                Some(value) => match args.reading_type {
                    ReadingType::HeartRate => println!("{value} bpm"),
                    ReadingType::Spo2 => println!("{value}%"),
                },
                None => {
                    println!("Error, no {type_label} detected. Is the ring being worn?");
                }
            }
        }
        Ok(Err(e)) => return Err(e.into()),
        Err(_elapsed) => {
            println!("Error, no {type_label} detected. Is the ring being worn?");
        }
    }

    Ok(())
}

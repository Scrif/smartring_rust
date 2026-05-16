use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use smartring_core::ble::get_default_adapter;
use smartring_core::client::Client;

mod commands;
mod output;

/// Interact with Colmi-family smart rings over Bluetooth LE.
#[derive(Debug, Parser)]
#[command(name = "smartring", version, about, long_about = None)]
struct Cli {
    /// Bluetooth address of the ring (preferred on Linux/Windows)
    #[arg(long, global = true)]
    address: Option<String>,

    /// Bluetooth device name of the ring (required on macOS; triggers a scan)
    #[arg(long, global = true)]
    name: Option<String>,

    /// Enable verbose BLE packet logging to stderr
    #[arg(long, global = true, default_value_t = false)]
    debug: bool,

    /// Append every received BLE packet to captures/colmi_response_capture_<ts>.bin
    #[arg(long, global = true, default_value_t = false)]
    record: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Discover nearby Colmi-compatible rings
    Scan(commands::scan::ScanArgs),

    /// Read the ring's automatic heart rate logging settings
    GetHeartRateLogSettings,

    /// Update the ring's automatic heart rate logging settings
    SetHeartRateLogSettings(commands::hr_settings::SetHrSettingsArgs),

    /// Print firmware version, hardware model, and battery level
    Info,

    /// Reboot the ring
    Reboot,

    /// Synchronise the ring's clock to the current UTC time
    SetTime(commands::set_time::SetTimeArgs),

    /// Fetch the heart rate log for a given date (defaults to today)
    GetHeartRateLog(commands::get_heart_rate_log::GetHeartRateLogArgs),

    /// Take a real-time sensor reading (heart-rate or spo2)
    GetRealTime(commands::get_real_time::GetRealTimeArgs),

    /// Sync ring data to a local SQLite database
    Sync(commands::sync::SyncArgs),

    /// Fetch sport-detail (step) data for a given date (defaults to today)
    GetSteps(commands::get_steps::GetStepsArgs),

    /// Send a raw packet and print the reply bytes as hex
    Raw(commands::raw::RawArgs),
}

/// Build a connected [`Client`] from the global `--address` / `--name` flags.
///
/// Errors if neither flag is provided (device commands require one or the other).
/// When `--record` is set, opens a binary capture file before returning.
async fn get_client(cli: &Cli) -> Result<Client> {
    let adapter = get_default_adapter()
        .await
        .context("failed to initialise Bluetooth adapter")?;

    let client = match (&cli.address, &cli.name) {
        (Some(addr), None) => Client::connect(&adapter, addr)
            .await
            .context("could not connect by address")?,
        (None, Some(name)) => Client::connect_by_name(&adapter, name)
            .await
            .context("could not connect by name")?,
        (None, None) => anyhow::bail!("--address or --name is required for this command"),
        (Some(_), Some(_)) => unreachable!("mutual exclusion already checked"),
    };

    if cli.record {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let path = std::path::PathBuf::from(format!(
            "captures/colmi_response_capture_{}.bin",
            ts
        ));
        eprintln!("Recording packets to {}", path.display());
        client.with_recording(path).context("failed to create capture file")
    } else {
        Ok(client)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing. In --debug mode show all spans; otherwise warnings only.
    let level = if cli.debug { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .with_writer(std::io::stderr)
        .init();

    // --address and --name are mutually exclusive.
    if cli.address.is_some() && cli.name.is_some() {
        anyhow::bail!("--address and --name are mutually exclusive; pass one or the other");
    }

    match cli.command {
        Commands::Scan(args) => commands::scan::run(args).await?,
        Commands::Info => {
            let client = get_client(&cli).await?;
            commands::info::run(&client).await?;
        }
        Commands::Reboot => {
            let client = get_client(&cli).await?;
            commands::reboot::run(&client).await?;
        }
        Commands::SetTime(ref args) => {
            let client = get_client(&cli).await?;
            commands::set_time::run(args, &client).await?;
        }
        Commands::GetHeartRateLogSettings => {
            let client = get_client(&cli).await?;
            commands::hr_settings::run_get(&client).await?;
        }
        Commands::SetHeartRateLogSettings(ref args) => {
            let client = get_client(&cli).await?;
            commands::hr_settings::run_set(args, &client).await?;
        }
        Commands::Sync(ref args) => {
            let client = get_client(&cli).await?;
            let addr = client.peripheral_address();
            commands::sync::run(args, &client, &addr, cli.name.as_deref()).await?;
        }
        Commands::GetHeartRateLog(ref args) => {
            let client = get_client(&cli).await?;
            commands::get_heart_rate_log::run(args, &client).await?;
        }
        Commands::GetRealTime(ref args) => {
            let client = get_client(&cli).await?;
            commands::get_real_time::run(args, &client).await?;
        }
        Commands::GetSteps(ref args) => {
            let client = get_client(&cli).await?;
            commands::get_steps::run(args, &client).await?;
        }
        Commands::Raw(ref args) => {
            let client = get_client(&cli).await?;
            commands::raw::run(args, &client).await?;
        }
    }

    Ok(())
}

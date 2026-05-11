use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use smartring_core::ble::get_default_adapter;
use smartring_core::client::Client;

mod commands;

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

    /// Enable verbose BLE packet logging
    #[arg(long, global = true, default_value_t = false)]
    debug: bool,

    /// Write all received BLE packets to a capture file in captures/
    #[arg(long, global = true, default_value_t = false)]
    record: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Discover nearby Colmi-compatible rings
    Scan(commands::scan::ScanArgs),

    /// Print firmware version, hardware model, and battery level
    Info,

    /// Reboot the ring
    Reboot,
}

/// Build a connected [`Client`] from the global `--address` / `--name` flags.
///
/// Errors if neither flag is provided (device commands require one or the other).
async fn get_client(cli: &Cli) -> Result<Client> {
    let adapter = get_default_adapter()
        .await
        .context("failed to initialise Bluetooth adapter")?;

    match (&cli.address, &cli.name) {
        (Some(addr), None) => Client::connect(&adapter, addr)
            .await
            .context("could not connect by address"),
        (None, Some(name)) => Client::connect_by_name(&adapter, name)
            .await
            .context("could not connect by name"),
        (None, None) => {
            anyhow::bail!("--address or --name is required for this command")
        }
        (Some(_), Some(_)) => unreachable!("mutual exclusion already checked"),
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
    }

    Ok(())
}

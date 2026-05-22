mod client;
mod config;
mod daemon_ctl;
mod env_cmd;
mod stats_cmd;

use anyhow::Result;
use clap::{Parser, Subcommand};
use flowy_config::{ensure_initialized, load_from_path};

/// CLI for Flowy Router — communicates with the `flowy-gateway` daemon over HTTP.
/// Configuration: `~/.flowy-router/config.toml` (all platforms).
#[derive(Debug, Parser)]
#[command(name = "flowy", version, about)]
struct Cli {
    /// Override path to config.toml (default: ~/.flowy-router/config.toml).
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print resolved paths and configuration.
    Env {
        #[arg(long)]
        json: bool,
    },
    /// Show gateway routing and traffic statistics.
    Stats {
        /// Cumulative totals from `stats.json` (includes history across restarts).
        #[arg(long)]
        global: bool,
        #[arg(long)]
        json: bool,
    },
    /// Manage the gateway daemon: start, stop, status, restart, run.
    #[command(subcommand)]
    Gateway(GatewayCommands),
}

#[derive(Debug, Subcommand)]
enum GatewayCommands {
    Start {
        #[arg(long, default_value_t = 30)]
        wait: u64,
    },
    Run,
    Stop {
        #[arg(short, long)]
        force: bool,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Restart {
        #[arg(long, default_value_t = 30)]
        wait: u64,
    },
}

fn ensure_settings(config_override: &Option<std::path::PathBuf>) -> Result<(config::CliSettings, bool)> {
    let (path, created) = ensure_initialized(config_override.as_deref())?;
    let (file, config_path) = load_from_path(&path)?;
    Ok((config::CliSettings { file, config_path }, created))
}

fn load_settings(config_override: &Option<std::path::PathBuf>) -> Result<config::CliSettings> {
    let path = match config_override {
        Some(p) => p.clone(),
        None => flowy_config::config_file()?,
    };
    let (file, config_path) = load_from_path(&path)?;
    Ok(config::CliSettings { file, config_path })
}

fn make_client(settings: &config::CliSettings) -> client::GatewayClient {
    client::GatewayClient::new(
        settings.gateway_url(),
        settings.api_key(),
        settings.admin_token(),
    )
}

fn print_init_message(created: bool, path: &std::path::Path) {
    if created {
        println!(
            "Created config at {} — edit upstream sections, then restart if needed.",
            path.display()
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_override = cli.config.clone();

    match cli.command {
        Commands::Env { json } => env_cmd::print_env(&config_override, json),
        Commands::Stats { global, json } => {
            stats_cmd::print_stats(&config_override, global, json).await
        }
        Commands::Gateway(cmd) => match cmd {
            GatewayCommands::Start { wait } => {
                let (settings, created) = ensure_settings(&config_override)?;
                print_init_message(created, &settings.config_path);
                let gw = make_client(&settings);
                daemon_ctl::start_daemon(&gw, &settings, wait).await
            }
            GatewayCommands::Run => {
                let (settings, created) = ensure_settings(&config_override)?;
                print_init_message(created, &settings.config_path);
                daemon_ctl::run_foreground(&settings).await
            }
            GatewayCommands::Stop { force } => {
                let settings = load_settings(&config_override)?;
                let gw = make_client(&settings);
                daemon_ctl::stop_daemon(&gw, force).await
            }
            GatewayCommands::Status { json } => {
                let settings = load_settings(&config_override)?;
                let gw = make_client(&settings);
                daemon_ctl::status_daemon(&gw, json).await
            }
            GatewayCommands::Restart { wait } => {
                let (settings, created) = ensure_settings(&config_override)?;
                print_init_message(created, &settings.config_path);
                let gw = make_client(&settings);
                daemon_ctl::restart_daemon(&gw, &settings, wait).await
            }
        },
    }
}

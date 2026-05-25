mod cli_settings;
mod client;
mod config;
mod daemon_ctl;
mod env_cmd;
mod gateway;
mod stats_cmd;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cli_settings::CliSettings;
use config::{ensure_initialized, load_from_path};
use gateway::{init_logging, AppConfig};
use tracing::info;

/// CLI for Flowy Router — gateway daemon and management commands.
/// Configuration: `~/.flowy-router/config.toml` (all platforms).
#[derive(Debug, Parser)]
#[command(name = "flowy", version, about)]
struct Cli {
    /// Override path to config.toml (default: ~/.flowy-router/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

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
        /// Human-readable output language: `en` (default) or `zh`.
        #[arg(long, default_value = "en", value_name = "LANG")]
        lang: String,
    },
    /// Manage the gateway daemon: start, stop, status, restart, run.
    #[command(subcommand)]
    Gateway(GatewayCommands),
    /// Hidden entry for the gateway daemon (re-invoked by `gateway start`).
    #[command(hide = true, name = "__serve")]
    Serve(ServeArgs),
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

#[derive(Debug, Parser)]
struct ServeArgs {
    /// Run in background daemon mode (write pid file).
    #[arg(long)]
    daemon: bool,

    /// Run in foreground (no pid file).
    #[arg(long)]
    foreground: bool,
}

fn ensure_settings(config_override: &Option<PathBuf>) -> Result<(CliSettings, bool)> {
    let (path, created) = ensure_initialized(config_override.as_deref())?;
    let (file, config_path) = load_from_path(&path)?;
    Ok((CliSettings { file, config_path }, created))
}

fn load_settings(config_override: &Option<PathBuf>) -> Result<CliSettings> {
    let path = match config_override {
        Some(p) => p.clone(),
        None => config::config_file()?,
    };
    let (file, config_path) = load_from_path(&path)?;
    Ok(CliSettings { file, config_path })
}

fn make_client(settings: &CliSettings) -> client::GatewayClient {
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

async fn run_serve(config_override: Option<PathBuf>, daemon: bool) -> Result<()> {
    let app_config = AppConfig::load_from(config_override.as_deref())?;

    let log_path = init_logging(&app_config.data_dir, !daemon)?;
    info!(
        config = %app_config.config_path.display(),
        app_dir = %app_config.data_dir.display(),
        log_file = %log_path.display(),
        "using config file"
    );

    if daemon {
        gateway::daemon::assert_not_running(&app_config)?;
        info!(pid_file = %app_config.pid_file.display(), "starting gateway daemon");
        return gateway::run(app_config, true).await;
    }

    info!("starting gateway in foreground");
    gateway::run(app_config, false).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_override = cli.config.clone();

    match cli.command {
        Commands::Serve(args) => run_serve(config_override, args.daemon).await,
        Commands::Env { json } => env_cmd::print_env(&config_override, json),
        Commands::Stats { global, json, lang } => {
            stats_cmd::print_stats(&config_override, global, json, &lang).await
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

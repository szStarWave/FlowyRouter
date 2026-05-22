use std::path::PathBuf;

use clap::Parser;
use flowy_gateway::config::AppConfig;
use flowy_gateway::daemon;
use flowy_gateway::init_logging;
use flowy_gateway::server;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "flowy-gateway", about = "Flowy Router gateway daemon")]
struct Args {
    /// Config file (default: ~/.flowy-router/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Run in background daemon mode (write pid file).
    #[arg(long)]
    daemon: bool,

    /// Run in foreground (no pid file).
    #[arg(long)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = AppConfig::load_from(args.config.as_deref())?;

    let log_path = init_logging(&config.data_dir, !args.daemon)?;
    info!(
        config = %config.config_path.display(),
        app_dir = %config.data_dir.display(),
        log_file = %log_path.display(),
        "using config file"
    );

    if args.daemon {
        daemon::assert_not_running(&config)?;
        info!(pid_file = %config.pid_file.display(), "starting gateway daemon");
        return server::run(config, true).await;
    }

    info!("starting gateway in foreground");
    server::run(config, false).await
}

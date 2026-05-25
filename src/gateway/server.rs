use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::gateway::api::router;
use crate::gateway::api::routes::AppState;
use crate::gateway::config::AppConfig;
use crate::gateway::daemon;
use crate::config::sessions_dir;

use crate::gateway::experience::ExperienceStore;
use crate::gateway::multimodal::MultimodalStore;
use crate::gateway::session::SessionStore;
use crate::gateway::stats::GatewayStats;
use crate::gateway::upstream::UpstreamClient;

pub struct GatewayRuntime {
    pub started_at: Instant,
    pub started_at_unix: u64,
    shutdown: watch::Sender<bool>,
}

impl GatewayRuntime {
    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    pub fn trigger_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

pub async fn run(config: AppConfig, register_pid: bool) -> anyhow::Result<()> {
    if register_pid {
        daemon::ensure_data_dir(&config)?;
        daemon::write_pid_file(&config)?;
    }

    let cancel = CancellationToken::new();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let runtime = Arc::new(GatewayRuntime {
        started_at: Instant::now(),
        started_at_unix: daemon::started_at_unix(),
        shutdown: shutdown_tx,
    });

    let stats = GatewayStats::open(&config.data_dir)?;
    stats.spawn_flush_task();

    let experience = ExperienceStore::open(&config.data_dir, config.experience.clone())?;
    experience.spawn_flush_task();

    let multimodal = MultimodalStore::open(&config.data_dir)?;
    multimodal.spawn_flush_task();

    let sessions_path = sessions_dir().unwrap_or_else(|_| config.data_dir.join("sessions"));
    let sessions = SessionStore::open(sessions_path, config.session_persist_enabled)?;
    sessions.spawn_flush_task();

    let sessions_for_shutdown = sessions.clone();
    let experience_for_shutdown = experience.clone();
    let multimodal_for_shutdown = multimodal.clone();
    let multimodal_for_upstream = multimodal.clone();
    let state = AppState {
        config: config.clone(),
        sessions,
        experience,
        multimodal,
        upstream: UpstreamClient::new(config.clone(), stats.clone(), multimodal_for_upstream),
        runtime: runtime.clone(),
        stats: stats.clone(),
    };

    let app = router(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let addr: SocketAddr = config.listen_addr.parse()?;
    let listener = TcpListener::bind(addr).await?;

    info!(%addr, "flowy gateway listening");
    info!(
        edge = config.edge_base_url.is_some(),
        cloud = config.cloud_base_url.is_some(),
        profile = ?config.default_profile,
        pid_file = %config.pid_file.display(),
        "gateway ready"
    );

    let cancel_serve = cancel.clone();
    let serve = async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_serve.cancelled().await;
            })
            .await
    };

    let mut shutdown_rx = shutdown_rx;
    let cancel_shutdown = cancel.clone();
    tokio::spawn(async move {
        let _ = shutdown_rx.changed().await;
        cancel_shutdown.cancel();
    });

    let ctrl_c = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("ctrl-c received, shutting down");
            ctrl_c.cancel();
        }
    });

    serve.await?;

    if let Err(e) = stats.flush() {
        tracing::warn!(error = %e, "final stats flush failed");
    }
    if let Err(e) = experience_for_shutdown.flush() {
        tracing::warn!(error = %e, "final experience flush failed");
    }
    if let Err(e) = multimodal_for_shutdown.flush() {
        tracing::warn!(error = %e, "final multimodal capability flush failed");
    }
    if let Err(e) = sessions_for_shutdown.flush() {
        tracing::warn!(error = %e, "final session flush failed");
    }

    if register_pid {
        daemon::remove_pid_file(&config);
    }

    Ok(())
}

#[allow(dead_code)]
pub fn app_router(state: AppState) -> Router {
    router(state)
}

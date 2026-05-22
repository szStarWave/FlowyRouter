use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use flowy_config::pid_file as config_pid_file;
use serde::Serialize;

use crate::client::GatewayClient;
use crate::config::CliSettings;

#[derive(Debug, Serialize)]
struct StoppedStatus {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct UnknownStatus {
    status: &'static str,
    pid: u32,
    note: &'static str,
}

pub fn read_pid() -> Option<u32> {
    let path = config_pid_file().ok()?;
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn resolve_gateway_bin(settings: &CliSettings) -> Result<PathBuf> {
    if let Some(p) = settings.gateway_bin() {
        return Ok(p);
    }

    let current = std::env::current_exe().context("current_exe")?;
    let sibling = current
        .parent()
        .map(|d| d.join("flowy-gateway"))
        .filter(|p| p.exists());
    if let Some(p) = sibling {
        return Ok(p);
    }

    which_flowy_gateway()
}

fn which_flowy_gateway() -> Result<PathBuf> {
    let out = Command::new("which")
        .arg("flowy-gateway")
        .output()
        .context("which flowy-gateway")?;
    if !out.status.success() {
        bail!(
            "flowy-gateway not found; run `cargo build -p flowy-gateway` or set cli.gateway_bin in ~/.flowy-router/config.toml"
        );
    }
    let path = String::from_utf8(out.stdout)?.trim().to_string();
    Ok(PathBuf::from(path))
}

pub async fn start_daemon(client: &GatewayClient, settings: &CliSettings, wait_secs: u64) -> Result<()> {
    if client.health().await.is_ok() {
        if let Ok(s) = client.status().await {
            println!(
                "gateway already running (pid {}, listen {})",
                s.pid, s.listen
            );
            return Ok(());
        }
        bail!("gateway already reachable at {}", client.base_url());
    }

    if let Some(pid) = read_pid() {
        if is_pid_alive(pid) {
            bail!("gateway already running (pid {pid})");
        }
        cleanup_stale_pid()?;
    }

    let bin = resolve_gateway_bin(settings)?;
    Command::new(&bin)
        .arg("--config")
        .arg(&settings.config_path)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", bin.display()))?;

    wait_until_healthy(client, wait_secs).await?;
    if let Ok(s) = client.status().await {
        println!(
            "gateway started (pid {}, listen {}, profile {})",
            s.pid, s.listen, s.default_profile
        );
    } else {
        println!("gateway started at {}", client.base_url());
    }
    Ok(())
}

pub async fn run_foreground(settings: &CliSettings) -> Result<()> {
    let bin = resolve_gateway_bin(settings)?;
    let status = Command::new(&bin)
        .arg("--config")
        .arg(&settings.config_path)
        .arg("--foreground")
        .status()
        .context("run flowy-gateway --foreground")?;
    if !status.success() {
        bail!("gateway exited with {status}");
    }
    Ok(())
}

pub async fn stop_daemon(client: &GatewayClient, force: bool) -> Result<()> {
    let pid = read_pid();
    let http_up = client.health().await.is_ok();

    if !http_up && pid.is_none() {
        println!("gateway is not running");
        return Ok(());
    }

    if http_up {
        let _ = client.shutdown().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        if client.health().await.is_err() {
            cleanup_stale_pid()?;
            println!("gateway stopped");
            return Ok(());
        }
    }

    if let Some(pid) = pid {
        if is_pid_alive(pid) {
            signal_stop(pid, force)?;
            for _ in 0..10 {
                if !is_pid_alive(pid) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        cleanup_stale_pid()?;
    }

    if client.health().await.is_ok() {
        bail!("gateway still reachable at {}", client.base_url());
    }

    println!("gateway stopped");
    Ok(())
}

pub async fn status_daemon(client: &GatewayClient, json: bool) -> Result<()> {
    match client.status().await {
        Ok(s) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_human_status(&s);
            }
            Ok(())
        }
        Err(_) => {
            if let Some(pid) = read_pid() {
                if is_pid_alive(pid) {
                    let u = UnknownStatus {
                        status: "unknown",
                        pid,
                        note: "process alive but HTTP unreachable",
                    };
                    if json {
                        println!("{}", serde_json::to_string_pretty(&u)?);
                    } else {
                        println!("Flowy Gateway");
                        println!("  Status:  unknown (pid {pid}, HTTP down)");
                        println!("  URL:     {}", client.base_url());
                    }
                    return Ok(());
                }
                cleanup_stale_pid()?;
            }
            let s = StoppedStatus { status: "stopped" };
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("Flowy Gateway");
                println!("  Status:  stopped");
                println!("  URL:     {}", client.base_url());
            }
            Ok(())
        }
    }
}

pub async fn restart_daemon(
    client: &GatewayClient,
    settings: &CliSettings,
    wait_secs: u64,
) -> Result<()> {
    let _ = stop_daemon(client, false).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    start_daemon(client, settings, wait_secs).await
}

fn print_human_status(s: &crate::client::GatewayStatus) {
    println!("Flowy Gateway");
    println!("  Status:   {}", s.status);
    println!("  Version:  {}", s.version);
    println!("  PID:      {}", s.pid);
    println!("  Listen:   {}", s.listen);
    println!("  Uptime:   {}s", s.uptime_secs);
    println!(
        "  Edge:     {}",
        if s.edge_configured {
            "configured"
        } else {
            "not configured"
        }
    );
    println!(
        "  Cloud:    {}",
        if s.cloud_configured {
            "configured"
        } else {
            "not configured"
        }
    );
    println!("  Profile:  {}", s.default_profile);
    println!("  PID file: {}", s.pid_file);
    println!("  Data dir: {}", s.data_dir);
}

async fn wait_until_healthy(client: &GatewayClient, secs: u64) -> Result<()> {
    for i in 0..secs {
        if client.health().await.is_ok() {
            return Ok(());
        }
        if i + 1 < secs {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    bail!(
        "gateway did not become healthy within {secs}s at {}",
        client.base_url()
    );
}

fn cleanup_stale_pid() -> Result<()> {
    if let Ok(path) = config_pid_file() {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn signal_stop(pid: u32, force: bool) -> Result<()> {
    #[cfg(unix)]
    {
        let sig = if force {
            libc::SIGKILL
        } else {
            libc::SIGTERM
        };
        let rc = unsafe { libc::kill(pid as i32, sig) };
        if rc != 0 {
            bail!("failed to signal pid {pid}");
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = force;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status();
        Ok(())
    }
}

use std::path::{Path, PathBuf};

/// User home directory (cross-platform).
///
/// - Linux / macOS: `$HOME`
/// - Windows: `%USERPROFILE%` / `dirs::home_dir()`
pub fn user_home() -> anyhow::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot resolve user home directory"))
}

/// Application root: `{user_home}/.flowy-router`
pub fn app_dir() -> anyhow::Result<PathBuf> {
    Ok(user_home()?.join(".flowy-router"))
}

pub fn config_file() -> anyhow::Result<PathBuf> {
    Ok(app_dir()?.join("config.toml"))
}

pub fn pid_file() -> anyhow::Result<PathBuf> {
    Ok(app_dir()?.join("gateway.pid"))
}

pub fn sessions_dir() -> anyhow::Result<PathBuf> {
    Ok(app_dir()?.join("sessions"))
}

pub fn logs_dir() -> anyhow::Result<PathBuf> {
    Ok(app_dir()?.join("logs"))
}

pub fn gateway_log_file() -> anyhow::Result<PathBuf> {
    Ok(logs_dir()?.join("gateway.log"))
}

pub fn stats_file() -> anyhow::Result<PathBuf> {
    Ok(app_dir()?.join("stats.json"))
}

pub fn ensure_app_dirs() -> anyhow::Result<PathBuf> {
    let root = app_dir()?;
    std::fs::create_dir_all(&root)?;
    std::fs::create_dir_all(sessions_dir()?)?;
    std::fs::create_dir_all(logs_dir()?)?;
    Ok(root)
}

pub fn display_home() -> String {
    user_home()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~".to_string())
}

pub fn display_app_dir() -> String {
    app_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| format!("{}/.flowy-router", display_home()))
}

pub fn is_under_app_dir(path: &Path) -> bool {
    app_dir()
        .ok()
        .is_some_and(|root| path.starts_with(&root))
}

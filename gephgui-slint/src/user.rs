use::std::path::PathBuf;

use anyhow::Ok;

use crate::manager::{self, AccountSecretCurrent, AccountSecretStatus, auth_secret};

#[cfg(target_os = "macos")]
fn settings_path() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        "/Library/Application Support/geph/settings.json",
    ))
}

#[cfg(target_os = "windows")]
fn settings_path() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        r"C:\ProgramData\geph\settings.json",
    ))
}

#[cfg(target_os = "linux")]
fn settings_path() -> anyhow::Result<PathBuf> {
    // Linux path of Geph settings.json
}

// Load secret from local store settings.json
pub fn load_secret() -> Option<String> {
    let path = settings_path().ok()?;

    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(path).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&content).ok()?;

    settings
        .get("secret")
        .and_then(|v| v.as_str())
        .map(String::from)
}

// Login if no available secret from settings.json
pub async fn user_login(secret: String) -> anyhow::Result<AccountSecretCurrent> {
    let status = auth_secret(secret.clone()).await?;
    match status {
        AccountSecretStatus::Current { current } => {
            // send secret to Daemon
            manager::secret_to_daemon(secret.clone()).await?;
            // acquire user_info for UI State
            // let user_info = fetch_user_info(secret.clone()).await?;
            Ok(current)
        }
        AccountSecretStatus::Invalid(status) => {
            Err(anyhow::anyhow!("Account secret is invalid: {}", status))
        }
        AccountSecretStatus::Retired(status) => {
            Err(anyhow::anyhow!("Account secret is retired: {}", status))
        }
    }
}

// log out by clearing the secret from Daemon and settings.json
pub async fn user_logout() -> anyhow::Result<()> {
    manager::logout().await?;
    Ok(())
}
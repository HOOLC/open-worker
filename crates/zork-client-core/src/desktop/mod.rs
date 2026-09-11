//! Native client host capabilities and configuration; independent of UI widgets.
pub mod account;
pub mod browser;
pub mod browser_worker;
pub mod directory;
pub mod node;
pub mod preview;
pub mod transport;
pub use zork_browser as browser_engine;
pub fn automation_token() -> String {
    zork_config::random_token()
}
use std::path::PathBuf;

pub fn client_root() -> PathBuf {
    std::env::var_os("ZORK_CLIENT_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Application Support/Zork/client")
        })
}

pub fn load_services() -> anyhow::Result<zork_config::services::ServicesConfig> {
    let bundled = std::env::current_exe()?
        .parent()
        .map(|p| p.join("../Resources/services.json"));
    let user = std::env::var_os("ZORK_SERVICES_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| client_root().join("services.json"));
    if std::env::var_os("ZORK_SERVICES_CONFIG").is_some() && !user.is_file() {
        anyhow::bail!("ZORK_SERVICES_CONFIG 文件不存在")
    }
    zork_config::services::ServicesConfig::load(bundled.as_deref(), Some(&user))
}

#[cfg(target_os = "macos")]
pub fn reduced_motion() -> bool {
    let reduced = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "com.apple.universalaccess", "reduceMotion"])
        .output()
        .ok()
        .is_some_and(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).trim() == "1");
    reduced
}

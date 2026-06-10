//! Persistent user settings: `~/.config/saladin/config.toml` (XDG). Loaded at
//! startup into a `UserConfig` resource; saved whenever a screen edits it.
//! Missing/partial files fall back to defaults field-by-field, so old configs
//! survive new fields.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    /// Shown to other players in multiplayer lobbies.
    pub player_name: String,
    /// Public relay for internet rooms (host your own: `saladin-server` on any
    /// VPS — see README). Both sides connect outbound, so no port forwarding.
    pub relay_addr: String,
    pub edge_scroll: bool,
    pub ui_scale: f32,
    pub master_volume: f32,
}

impl Default for UserConfig {
    fn default() -> Self {
        UserConfig {
            player_name: String::new(),
            relay_addr: "127.0.0.1:5000".into(),
            edge_scroll: true,
            ui_scale: 1.0,
            master_volume: 1.0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn config_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(
        || {
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
            home.join(".config")
        },
    );
    base.join("saladin/config.toml")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> UserConfig {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save(cfg: &UserConfig) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match toml::to_string_pretty(cfg) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&path, s) {
                eprintln!("config save failed: {e}");
            }
        }
        Err(e) => eprintln!("config serialize failed: {e}"),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load() -> UserConfig {
    UserConfig::default()
}

#[cfg(target_arch = "wasm32")]
pub fn save(_cfg: &UserConfig) {}

/// Non-loopback IPv4 addresses of this machine, for the "read this to your
/// friend" line on the LAN host screen.
#[cfg(not(target_arch = "wasm32"))]
pub fn lan_ips() -> Vec<String> {
    match local_ip_address::list_afinet_netifas() {
        Ok(ifas) => {
            let mut ips: Vec<String> = ifas
                .into_iter()
                .filter(|(_, ip)| ip.is_ipv4() && !ip.is_loopback())
                .map(|(_, ip)| ip.to_string())
                .collect();
            // home-router ranges first; docker/VPN bridges (172.16-31.*) last
            ips.sort_by_key(|ip| {
                if ip.starts_with("192.168.") {
                    0
                } else if ip.starts_with("10.") {
                    1
                } else {
                    2
                }
            });
            ips.truncate(3);
            ips
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn lan_ips() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_toml() {
        let cfg = UserConfig {
            player_name: "Saladin".into(),
            relay_addr: "vps.example.com:5000".into(),
            edge_scroll: false,
            ui_scale: 1.25,
            master_volume: 0.4,
        };
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: UserConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.player_name, "Saladin");
        assert_eq!(back.relay_addr, "vps.example.com:5000");
        assert!(!back.edge_scroll);
        assert_eq!(back.ui_scale, 1.25);
        assert_eq!(back.master_volume, 0.4);
    }

    #[test]
    fn partial_files_fall_back_per_field() {
        let back: UserConfig = toml::from_str("player_name = \"Just A Name\"\n").unwrap();
        assert_eq!(back.player_name, "Just A Name");
        assert_eq!(back.relay_addr, UserConfig::default().relay_addr);
        assert!(back.edge_scroll);
    }
}

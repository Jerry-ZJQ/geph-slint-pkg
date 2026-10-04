use geph5_misc_rpc::manager_control::{TunnelSettings};
use geph5_broker_protocol::{ExitCategory, ExitConstraint, NetStatus, UserInfo, AccountLevel};
use geph5_misc_rpc::client_control::ConnInfo;

use chrono::{DateTime, Local};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::{BTreeMap, HashMap};


// supporting functions for RpcRawStates impl
fn current_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn map_exit_category(category: &ExitCategory) -> UiExitCategory {
    match category {
        ExitCategory::Core => UiExitCategory::Core,
        ExitCategory::Streaming => UiExitCategory::Streaming,
    }
}

fn map_load_level(load: f32) -> ServerLoadLevel {
    if load < 0.50 {
        ServerLoadLevel::Low
    } else if load <= 0.80 {
        ServerLoadLevel::Medium
    } else {
        ServerLoadLevel::High
    }
}

fn is_server_enabled(
    allowed_levels: &[AccountLevel],
    account_level: &str,
) -> bool {
    match account_level {
        "Free" => allowed_levels.contains(&AccountLevel::Free),
        "Plus" => {
            allowed_levels.contains(&AccountLevel::Free)
                || allowed_levels.contains(&AccountLevel::Plus)
        }
        _ => false,
    }
}

// fn category_sort_key(category: &UiExitCategory) -> u8 {
//     match category {
//         UiExitCategory::Core => 0,
//         UiExitCategory::Streaming => 1,
//     }
// }

fn map_session_metadata(
    metadata: &serde_json::Value,
) -> (bool, bool) {
    let ads_filter = metadata
        .get("filter")
        .and_then(|filter| filter.get("ads"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let adult_filter = metadata
        .get("filter")
        .and_then(|filter| filter.get("nsfw"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    (ads_filter, adult_filter)
}


// Raw Data model acquired via daemon_rpc
#[derive(Clone)]
pub struct RpcRawStates {
    pub user_info: UserInfo,
    pub conn_info: ConnInfo,
    pub tunnel_settings: TunnelSettings,
    pub net_status: NetStatus,
}

impl Default for RpcRawStates {
    fn default() -> Self {
        Self {
            conn_info: ConnInfo::Disconnected,

            user_info: UserInfo {
                user_id: 0,
                plus_expires_unix: None,
                recurring: false,
                bw_consumption: None,
            },

            tunnel_settings: TunnelSettings { 
                exit_constraint: ExitConstraint::Auto, 
                vpn: true, 
                allow_lan: false,
                allow_direct: false,
                passthrough_china: false,
                session_metadata: Value::Null,
                proxy: None,
            },

            net_status: NetStatus {
                exits: BTreeMap::new(), 
            },
        }
        
    }
}

impl RpcRawStates {
    // Convert RPC UserInfo data into the format for Slint UI consumption
    pub fn to_ui_user_info(&self) -> UiUserInfo {
        let now = current_unix();

        let (account_level, expire_date, remaining_days) =
            match self.user_info.plus_expires_unix {
                Some(expires) if expires > now => {
                    let expire_date = DateTime::from_timestamp(expires as i64, 0)
                        .map(|dt| {
                            dt.with_timezone(&Local)
                                .format("%Y-%m-%d")
                                .to_string()
                        })
                        .unwrap_or_else(|| "N/A".to_string());

                    let remaining_days =
                        ((expires - now) / 86_400).to_string();

                    (
                        "Plus".to_string(),
                        expire_date,
                        remaining_days,
                    )
                }

                _ => (
                    "Free".to_string(),
                    "N/A".to_string(),
                    "N/A".to_string(),
                ),
            };

        let (remaining_usage, remaining_percent, renew_date) =
            match self.user_info.bw_consumption {
                Some(bw) => {
                    let remaining_mb = bw.mb_limit.saturating_sub(bw.mb_used);

                    let percent = if bw.mb_limit > 0 {
                        remaining_mb as f32 / bw.mb_limit as f32
                    } else {
                        0.0
                    };

                    let date = chrono::DateTime::from_timestamp(bw.renew_unix as i64, 0)
                        .map(|dt| {
                            dt.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d")
                                .to_string()
                        })
                        .unwrap_or_else(|| "N/A".to_string());

                    (
                        format!("{} / {} MB", remaining_mb, bw.mb_limit),
                        percent,
                        date,
                    )
                }

                None => (
                    "Unlimited".to_string(),
                    1.0,
                    "N/A".to_string(),
                ),
            };

        UiUserInfo {
            // account_secret: String::new(),
            account_level,
            expire_date,
            remaining_days,
            remaining_usage,
            remaining_percent,
            renew_date,
        }
    }

    // Convert NetStatus raw data into UiServerEntry/UiServerList model for Slint UI consumpion
    pub fn to_ui_server_sections(
        &self,
        account_level: &str,
    ) -> UiServerSections {
        let mut servers: HashMap<
            (String, String, UiExitCategory),
            (f32, Vec<AccountLevel>),
        > = HashMap::new();

        for (_hostname, (_pk, descriptor, metadata))
            in &self.net_status.exits
        {
            let country = descriptor.country.alpha2().to_string();
            let city = descriptor.city.clone();
            let category = map_exit_category(&metadata.category);

            let key = (
                country,
                city,
                category,
            );

            match servers.get_mut(&key) {
                Some((min_load, _allowed_levels)) => {
                    if descriptor.load < *min_load {
                        *min_load = descriptor.load;
                    }
                }

                None => {
                    servers.insert(
                        key,
                        (
                            descriptor.load,
                            metadata.allowed_levels.clone(),
                        ),
                    );
                }
            }
        }

        let mut core_servers = Vec::new();
        let mut streaming_servers = Vec::new();

        for ((country, city, category), (load, allowed_levels))
            in servers
        {
            let entry = UiServerEntry {
                country,
                city,
                load: format!(
                    "{:.0}%",
                    (load * 100.0).floor()
                ),
                load_level: map_load_level(load),
                enabled: is_server_enabled(
                    &allowed_levels,
                    account_level,
                ),
            };

            match category {
                UiExitCategory::Core => {
                    core_servers.push(entry);
                }

                UiExitCategory::Streaming => {
                    streaming_servers.push(entry);
                }
            }
        }

        let sort_servers = |a: &UiServerEntry, b: &UiServerEntry| {
            (a.country.as_str(), a.city.as_str())
                .cmp(&(b.country.as_str(), b.city.as_str()))
        };

        core_servers.sort_by(sort_servers);
        streaming_servers.sort_by(sort_servers);

        vec![
            UiServerSection {
                category: UiExitCategory::Core,
                title: "Core".to_string(),
                helptext: String::new(),
                servers: core_servers,
            },

            UiServerSection {
                category: UiExitCategory::Streaming,
                title: "Streaming".to_string(),
                helptext: String::new(),
                servers: streaming_servers,
            },
        ]
    }

    // Convert raw ExitConstraint to UI Data model for Slint
    pub fn to_ui_server_selection(&self) -> UiExitSelection {
        match &self.tunnel_settings.exit_constraint {
            ExitConstraint::Auto => {
                UiExitSelection::Auto
            }

            ExitConstraint::CountryCity(country, city) => {
                UiExitSelection::Manual {
                    country: country.alpha2().to_string(),
                    city: city.clone(),
                }
            }

            _ => {
                println!(
                    "Unsupported ExitConstraint for UI server selection: {:?}; \
                     falling back to Auto",
                    self.tunnel_settings.exit_constraint
                );
                UiExitSelection::Auto
            }
        }
    }

    // Convert raw TunnelSetting/ProxySetting to UI data model for Slint
    pub fn to_ui_tunnel_settings(&self) -> UiTunnelSettings {
        let (ads_filter, adult_filter) =
            map_session_metadata(
                &self.tunnel_settings.session_metadata
            );

        let proxy_settings = match &self.tunnel_settings.proxy {
            Some(proxy) => UiProxySettings {
                enabled: true,
                socks5_port: proxy.socks5_port,
                http_port: proxy.http_port,
                listen_all: proxy.listen_all,
                auto_config: proxy.autoconf,
            },

            None => UiProxySettings {
                enabled: false,
                socks5_port: 0,
                http_port: 0,
                listen_all: false,
                auto_config: false,
            },
        };

        UiTunnelSettings {
            allow_direct: self.tunnel_settings.allow_direct,
            ads_filter,
            adult_filter,
            allow_lan: self.tunnel_settings.allow_lan,
            bypass_prc: self.tunnel_settings.passthrough_china,
            global_vpn: self.tunnel_settings.vpn,
            proxy_settings,
        }
    }

    // Convert Raw ConnInfo to Slint UI data model
    pub fn to_ui_conn_info(&self) -> UiConnInfo {
        match &self.conn_info {
            ConnInfo::Disconnected => UiConnInfo {
                state: UiConnState::Disconnected,
                country: String::new(),
                city: String::new(),
                exit: String::new(),
                bridge: String::new(),
                protocol: String::new(),
            },

            ConnInfo::Connecting => UiConnInfo {
                state: UiConnState::Connecting,
                country: String::new(),
                city: String::new(),
                exit: String::new(),
                bridge: String::new(),
                protocol: String::new(),
            },

            ConnInfo::Connected { sessions } => {
                if sessions.is_empty() {
                    return UiConnInfo {
                        state: UiConnState::Connecting,
                        country: String::new(),
                        city: String::new(),
                        exit: String::new(),
                        bridge: String::new(),
                        protocol: String::new(),
                    };
                }

                // Find the country with the most sessions.
                let mut counts = std::collections::HashMap::new();

                for session in sessions {
                    *counts.entry(session.exit.country).or_insert(0usize) += 1;
                }

                let best_country = counts
                    .into_iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(country, _)| country)
                    .unwrap();

                // Keep the first session belonging to the primary country.
                let primary = sessions
                    .iter()
                    .find(|session| session.exit.country == best_country)
                    .unwrap();

                UiConnInfo {
                    state: UiConnState::Connected,
                    country: primary.exit.country.alpha2().to_string(),
                    city: primary.exit.city.clone(),
                    exit: primary.exit.c2e_listen.ip().to_string(),
                    bridge: primary
                        .bridge
                        .map(|addr| addr.to_string())
                        .unwrap_or_default(),
                    protocol: primary.protocol.clone(),
                }
            }
        }
    }
}

// Data model for Slint UI consumption
#[derive(Clone, Debug, PartialEq)]
pub struct UiUserInfo {
    // account_secret: String,
    pub account_level: String,
    pub expire_date: String,
    pub remaining_days: String,
    pub remaining_usage: String,
    pub remaining_percent: f32,
    pub renew_date: String,
}

impl Default for UiUserInfo {
    fn default() -> Self {
        Self {
            account_level: String::new(),
            expire_date: String::new(),
            remaining_days: String::new(),
            remaining_usage: String::new(),
            remaining_percent: 1.0,
            renew_date: String::new(),
        }
    }
}

#[derive(Clone, Debug, Copy, Eq, Hash, PartialEq)]
pub enum ServerLoadLevel {
    Low,
    Medium,
    High
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UiExitCategory {
    Core,
    Streaming,
}

// Data model for ServerList Entry items in Slint UI
#[derive(Clone, Eq, Debug, Hash, PartialEq)]
pub struct UiServerEntry {
    // server_id: String,
    // category: UiExitCategory,
    pub country: String,
    pub city: String,
    pub load: String,
    pub load_level: ServerLoadLevel,
    pub enabled: bool
}

#[derive(Clone, Eq, Debug, Hash, PartialEq)]
pub struct UiServerSection {
    pub category: UiExitCategory,
    pub title: String,
    pub helptext: String,
    pub servers: Vec<UiServerEntry>,
}

pub type UiServerSections = Vec<UiServerSection>;


// Data model for selected server in Slint UI - mapping ExitConstraint in RPC raw data
#[derive(Clone, Eq, Debug, Hash, PartialEq)]
pub enum UiExitSelection {
    Auto,
    Manual {
        country: String,
        city: String,
    }
}

// Data model for tunnel setting item states in Slint UI 
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiTunnelSettings {
    // exit_constraint: String,
    pub allow_direct: bool,
    pub ads_filter: bool,
    pub adult_filter: bool,
    pub allow_lan: bool,
    pub bypass_prc: bool,
    pub global_vpn: bool,
    pub proxy_settings: UiProxySettings,
}

impl Default for UiTunnelSettings {
    fn default() -> Self {
        Self {
            allow_direct: false,
            ads_filter: false,
            adult_filter: false,
            allow_lan: false,
            bypass_prc: false,
            global_vpn: true,
            proxy_settings: UiProxySettings::default(),
        }
    }
}

// Date model for proxy setting item states in Slint UI
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiProxySettings {
    pub enabled: bool,
    pub socks5_port: u16,
    pub http_port: u16,
    pub listen_all: bool,
    pub auto_config: bool,
}

impl Default for UiProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_config: true,
            listen_all: false,
            socks5_port: 9909,
            http_port: 9910,
        }
    }
}

// Data model of ConnInfo (connection status) for Slint UI
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiConnState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiConnInfo {
    pub state: UiConnState,
    pub country: String,
    pub city: String,
    pub exit: String,
    pub bridge: String,
    pub protocol: String,
}

impl Default for UiConnInfo {
    fn default() -> Self {
        Self {
            state: UiConnState::Disconnected,
            country: String::new(),
            city: String::new(),
            exit: String::new(),
            bridge: String::new(),
            protocol: String::new(),
        }
    }
}

// Language setting UI data model
// #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
// pub enum LanguageOption {
//     English,
//     SimplifiedChinese,
//     TraditionalChinese,
//     Russian,
//     Arabic,
//     Farsi,
//     Ukrainian,
// }

// #[derive(Clone, Eq, Debug, Hash, PartialEq)]
// pub struct LanguageItem {
//     pub language: LanguageOption,
//     pub display_name: String,
// }

// pub type LanguageItems = Vec<LanguageItem>;
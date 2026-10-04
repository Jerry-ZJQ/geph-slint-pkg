use::std::sync::Arc;

use geph5_misc_rpc::manager_control::ProxySettings;

use crate::ui_states::{UiExitSelection, UiConnState};
use crate::ui_service::{UiService};
use crate::manager;

use crate::{GephTray, SettingActions};

// tray callback register
pub fn register_callbacks(tray: &GephTray, ui_svc: Arc<UiService>) {
    let service = Arc::clone(&ui_svc);

    // Callback process for Connection controls
    let conn_service = service.clone();
    tray.on_toggle_connect(move || {
        let conn_service = Arc::clone(&conn_service);
        let service = conn_service.clone();

        geph5_rt::spawn(async move {
            let conn_info = conn_service.current_conn_info().await;
            let settings = conn_service.current_tunnel_settings().await;

            match conn_info.state {
                UiConnState::Disconnected => {
                    // Connect
                    println!("[Toggle Connect Action]: Connect");

                    let secret = conn_service.current_secret().await;
                    if let Err(e) =
                        manager::start_daemon(secret, settings).await                   
                    {
                        println!("[Tray] Start daemon failed: {e:?}");
                    }
                }
                UiConnState::Connecting => {
                    // Cancel or disconnect
                    println!("[Toggle Connect Action]: Cancel");

                    if let Err(e) =
                        manager::stop_daemon().await
                    {
                        println!("[Tray] Stop daemon failed: {e:?}");
                    }
                }
                UiConnState::Connected => {
                    // Disconnect
                    println!("[Toggle Connect Action]: Disconnect");

                    if let Err(e) =
                        manager::stop_daemon().await
                    {
                        println!("[Tray] Stop daemon failed: {e:?}");
                    }
                }
            }

        }).detach();
        // poll immediately to set origin state for burst
        service.poll_conn_info();
    });

    // Callback process for exit server auto selected
    let auto_service = service.clone();
    tray.on_auto_selected(move || {
        let auto_service = Arc::clone(&auto_service);
        let service = auto_service.clone();

        println!("[Tray Callback] Auto selected");

        geph5_rt::spawn(async move {
            let conn_info = auto_service.current_conn_info().await;

            match conn_info.state {
                UiConnState::Disconnected => {
                    if let Err(e) = 
                    manager::set_exit_constraint(&UiExitSelection::Auto).await {
                        println!("[Tray] Set Auto ExitConstraint failed: {e:?}");
                    }
                }
                _ => {
                    // set ExitConstraint as part of TunnelSettings and restart Daemon
                    let mut settings = auto_service.current_tunnel_settings().await;

                    settings.exit_constraint = geph5_broker_protocol::ExitConstraint::Auto;

                    if let Err(e) = 
                    manager::restart_daemon(settings).await {
                        println!("[Tray] Restart Daemon failed for server change during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // poll tunnel settings to set up origin state for burst
        service.poll_tunnel_settings();
    });

    // Callback process for exit server manually selected
    let exit_service = service.clone();
    tray.on_exit_selected(move |country, city| {
        let exit_service = Arc::clone(&exit_service);
        let service = exit_service.clone();

        println!("[Tray Callback] Exit selected: {country}, {city}");

        let ui_country = country.to_string();
        let ui_city = city.to_string();

        let exit = UiExitSelection::Manual {
            country: ui_country.clone(),
            city: ui_city.clone(), 
        };

        geph5_rt::spawn(async move {
            let conn_info = exit_service.current_conn_info().await;

            match conn_info.state {
                UiConnState::Disconnected => {
                    if let Err(e) = 
                    manager::set_exit_constraint(&exit).await {
                        println!("[Tray] Set Manual ExitConstraint failed: {e:?}");
                    }
                }
                _ => {
                    // set ExitConstraint as part of TunnelSettings and restart Daemon
                    let mut settings = exit_service.current_tunnel_settings().await;

                    settings.exit_constraint = match manager::exit_constraint(&exit) {
                        Ok(value) => value,
                        Err(e) => {
                            println!("[Window] Set Manual ExitConstraint failed: {e:?}");
                            return;
                        }
                    };

                    if let Err(e) = 
                    manager::restart_daemon(settings).await {
                        println!("[Tray] Restart Daemon failed for server change during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // poll tunnel settings to trigger burst
        service.poll_tunnel_settings();
    });

    // Callback process for TunnelSetting changes
    // Tray doesn't include all setting items 
    // but still reserved all processes except ProxySettings items
    let setting_service = service.clone();

    tray.on_setting_requested(move |action| {
        println!("[Tray Callback] TunnelSetting changed");
        let setting_service = Arc::clone(&setting_service);
        // let service = setting_service.clone();
        
        geph5_rt::spawn(async move {
            let conn_info = setting_service.current_conn_info().await;
            let mut settings = setting_service.current_tunnel_settings().await;

            match action {
                SettingActions::AllowDirect => {
                    settings.allow_direct = !settings.allow_direct;
                }
                SettingActions::AdsFilter => {
                    let current = settings
                        .session_metadata
                        .get("filter")
                        .and_then(|v| v.get("ads"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    settings.session_metadata["filter"]["ads"] =
                        serde_json::json!(!current);
                }
                SettingActions::AdultFilter => {
                    let current = settings
                        .session_metadata
                        .get("filter")
                        .and_then(|v| v.get("nsfw"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    settings.session_metadata["filter"]["nsfw"] =
                        serde_json::json!(!current);
                }
                SettingActions::BypassLan => {
                    settings.allow_lan = !settings.allow_lan;
                }
                SettingActions::BypassChina => {
                    settings.passthrough_china = !settings.passthrough_china;
                }
                SettingActions::GlobalVPN => {
                    settings.vpn = !settings.vpn;

                    // Global VPN and Local Proxy cannot both be OFF.
                    if !settings.vpn && settings.proxy.is_none() {
                        settings.proxy = match settings.proxy {
                            Some(_) => None,
                            // restore default proxy if the LocalProxy switch turned on
                            None => Some(ProxySettings {
                                autoconf: true,
                                listen_all: false,
                                socks5_port: 9909,
                                http_port: 9910,
                            }),
                        };
                    }
                }
                // ProxySettings are ignored for Tray
                _ => {}
            }

            // If Connected or Connecting, restart Daemon with changed settings
            // If Disconnected, apply setting change to Daemon only (no re-connect)
            match conn_info.state {
                UiConnState::Disconnected => {
                    if let Err(e) = 
                    manager::apply_tunnel_settings(settings).await {
                        println!("[Tray] Set TunnelSettings failed: {e:?}");
                    }
                }
                _ => {
                    if let Err(e) =
                    manager::restart_daemon(settings).await {
                        println!("[Tray] Set TunnelSettings failed during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // no busrt polling call to reserve a debounce for setting items 
        // service.poll_tunnel_settings();
    });
}
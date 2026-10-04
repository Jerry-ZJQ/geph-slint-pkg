use std::sync::Arc;

use crate::user::{user_login, user_logout};
use crate::{GephWindow, SettingActions};
use crate::ui_states::{
    UiConnState, 
    UiExitSelection,
};

use crate::ui_service::UiService;
use crate::manager::{self};
use geph5_misc_rpc::manager_control::{ProxySettings};
use crate::LoginState;


// Register callback for Slint
pub fn register_callbacks(window: &GephWindow, ui_svc: Arc<UiService>) {
    
    let service = Arc::clone(&ui_svc);

    // callback process for toggle_connect (Connect Button click)
    let conn_service = service.clone();
    window.on_toggle_connect(move || {
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
                        println!("[Window] Start daemon failed: {e:?}");
                    }
                }
                UiConnState::Connecting => {
                    // Cancel or disconnect
                    println!("[Toggle Connect Action]: Cancel");

                    if let Err(e) =
                        manager::stop_daemon().await
                    {
                        println!("[Window] Stop daemon failed: {e:?}");
                    }
                }
                UiConnState::Connected => {
                    // Disconnect
                    println!("[Toggle Connect Action]: Disconnect");

                    if let Err(e) =
                        manager::stop_daemon().await
                    {
                        println!("[Window] Stop daemon failed: {e:?}");
                    }
                }
            }

        }).detach();
        // poll immediately to trigger burst
        service.poll_conn_info();
    });

    // Callback process for automatic connect selected
    let auto_service = service.clone();
    window.on_auto_selected(move || {
        let auto_service = Arc::clone(&auto_service);
        let service = auto_service.clone();
        println!("[Window Callback] Auto selected");

        geph5_rt::spawn(async move {
            let conn_info = auto_service.current_conn_info().await;

            match conn_info.state {
                UiConnState::Disconnected => {
                    if let Err(e) = 
                    manager::set_exit_constraint(&UiExitSelection::Auto).await {
                        println!("[Window] Set Auto ExitConstraint failed: {e:?}");
                    }
                }
                _ => {
                    // set ExitConstraint as part of TunnelSettings and restart Daemon
                    let mut settings = auto_service.current_tunnel_settings().await;

                    settings.exit_constraint = geph5_broker_protocol::ExitConstraint::Auto;

                    if let Err(e) = 
                    manager::restart_daemon(settings).await {
                        println!("[Window] Restart Daemon failed for server change during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // poll tunnel settings to trigger burst polling for fast UI response
        service.poll_tunnel_settings();
    });

    // Callback process for exit server selected
    let exit_service = service.clone();
    window.on_exit_selected(move |country, city| {
        let exit_service = Arc::clone(&exit_service);
        let service = exit_service.clone();

        println!("[Window Callback] Exit selected: {country}, {city}");

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
                        println!("[Window] Set Manual ExitConstraint failed: {e:?}");
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
                        println!("[Window] Restart Daemon failed for server change during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // poll tunnel settings to trigger burst polling for fast UI response
        service.poll_tunnel_settings();
    });

    // Callback process for TunnelSetting changes
    let setting_service = service.clone();
    window.on_setting_requested(move |action, value| {
        println!("[Window Callback] TunnelSetting changed");
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
                SettingActions::LocalProxy => {
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
                    // Global VPN and Local Proxy cannot both be OFF.
                    if settings.proxy.is_none() && !settings.vpn {
                        settings.vpn = true;
                    }
                }
                SettingActions::AutoConfig => {
                    if let Some(proxy) = settings.proxy.as_mut() {
                        proxy.autoconf = !proxy.autoconf;
                    }
                }
                SettingActions::ListenAll => {
                    if let Some(proxy) = settings.proxy.as_mut() {
                        proxy.listen_all = !proxy.listen_all;
                    }
                }
                SettingActions::Sock5Port => {
                    if let Some(proxy) = settings.proxy.as_mut() {
                        proxy.socks5_port = value as u16;
                    }
                }
                SettingActions::HttpPort => {
                    if let Some(proxy) = settings.proxy.as_mut() {
                        proxy.http_port = value as u16;
                    }
                }
            }

            // If Connected or Connecting, restart Daemon with changed settings
            // If Disconnected, apply setting change to Daemon only (no re-connect)
            match conn_info.state {
                UiConnState::Disconnected => {
                    if let Err(e) = 
                    manager::apply_tunnel_settings(settings).await {
                        println!("[Window] Set TunnelSettings failed: {e:?}");
                    }
                }
                _ => {
                    if let Err(e) =
                    manager::restart_daemon(settings).await {
                        println!("[Window] Set TunnelSettings failed during connection: {e:?}");
                    }
                }
            }
        }).detach();
        // no busrt polling call to reserve a debounce for setting items 
        // service.poll_tunnel_settings();
    });

    // callback for user login
    let login_service = Arc::clone(&service);

    window.on_login_requested(move |secret| {
        let login_service = Arc::clone(&login_service);
        let secret = secret.to_string();

        geph5_rt::spawn(async move {

            login_service.set_login_state(LoginState::Authenticating).await;

            match user_login(secret.clone()).await {

                Ok(secret_info) => {
                    println!("[Window] Login succeeded: {secret_info:?}");

                    login_service.set_secret(secret).await;

                    login_service
                        .set_login_state(LoginState::LoggedIn)
                        .await;
                    // poll UserInfo right after login success to update UI
                    login_service.poll_user_info();
                }

                Err(e) => {
                    println!("[Window] Login failed: {e:?}");
                    login_service.set_login_state(LoginState::LoggedOut).await;
                }
            }
        }).detach();
    });

    // callback for user logout
    let logout_service = service.clone();
    window.on_logout_requested(move || {
        let service = Arc::clone(&logout_service);
        geph5_rt::spawn(async move {
            if let Err(e) = user_logout().await {
                println!("[Window] Logout failed: {e:?}");
                return;
            }
            // Clear secret and UIService state cache as well
            service.clear_secret().await;
            service.clear_service().await;
            service.set_login_state(LoginState::LoggedOut).await;
            service.poll_all();
        }).detach();
    });

    // callback for server list popup and trigger immediate server list refresh
    // let exit_refresh_service = service.clone();
    // window.on_server_list_opened(move || {
    //     let service = Arc::clone(&exit_refresh_service);
    //     service.poll_net_status();
    // });
}
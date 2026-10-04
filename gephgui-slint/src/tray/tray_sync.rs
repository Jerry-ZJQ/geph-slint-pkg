use::std::sync::Arc;

use crate::ui_states::{
    UiConnInfo,
    UiExitSelection,
    UiServerSections,
    UiTunnelSettings,
};
use crate::ui_service::UiService;

use crate::{GephTray};
use crate::{
    ConnInfo,
    SelectedServer,
    ServerEntry,
    ServerSection,
    ServerList,
    TunnelSettings,
    ProxySettings,
    LoginState,
    AccountStatus
};

use slint::{ModelRc, VecModel};

use crate::window::window_sync::{to_slint_load_level, to_slint_category, to_slint_conn_state};

// Sync LoginState to Slint Tray
fn sync_login_state(tray: &GephTray, state: LoginState) {
    tray
    .global::<AccountStatus>()
    .set_login_status(state.into());
}

// Sync ConnInfo to Slint Tray
fn sync_conn_info(tray: &GephTray, info: &UiConnInfo) {

    let conn = tray.global::<ConnInfo>();

    conn.set_conn_state(to_slint_conn_state(&info.state));
    conn.set_country(info.country.clone().into());
    conn.set_city(info.city.clone().into());
    conn.set_exit_ip(info.exit.clone().into());
    conn.set_bridge_ip(info.bridge.clone().into());
    conn.set_protocol(info.protocol.clone().into());
}

// Sync Settings to Slint Tray
pub fn sync_settings(tray: &GephTray, settings: &UiTunnelSettings) {
    let tunnel_settings = tray.global::<TunnelSettings>();
    let proxy_settings = tray.global::<ProxySettings>();

    tunnel_settings.set_allow_direct(settings.allow_direct);
    tunnel_settings.set_global_vpn(settings.global_vpn);
    tunnel_settings.set_bypass_china(settings.bypass_prc);
    tunnel_settings.set_bypass_lan(settings.allow_lan);
    tunnel_settings.set_ads_filter(settings.ads_filter);
    tunnel_settings.set_adult_filter(settings.adult_filter);

    proxy_settings.set_enabled(settings.proxy_settings.enabled);
    proxy_settings.set_auto_config(settings.proxy_settings.auto_config);
    proxy_settings.set_listening_all(settings.proxy_settings.listen_all);
    proxy_settings.set_socks5_port(settings.proxy_settings.socks5_port as i32);
    proxy_settings.set_http_port(settings.proxy_settings.http_port as i32);
}

// Sync Server/Exit selection to Slint Tray
fn sync_exit_selection(tray: &GephTray, selection: &UiExitSelection) {

    let server = tray.global::<SelectedServer>();
    // println!("[sync_exit_selection entered.");
    
    match selection {
        UiExitSelection::Auto => {
            server.set_country("Auto".into());
            server.set_city("".into());
        }
        UiExitSelection::Manual { country, city } => {
            server.set_country(country.clone().into());
            server.set_city(city.clone().into());
        }
    }
}

// Sync server section/list to Slint Tray
// Plus only servers are filtered for Free users and will not be synced to Tray
fn sync_server_sections(tray: &GephTray, sections: &UiServerSections) {
    let server_list = tray.global::<ServerList>();

    let section_model = VecModel::from(
        sections
            .iter()
            .map(|section| {
                let server_model = VecModel::from(
                    section
                        .servers
                        .iter()
                        .filter(|server| server.enabled)
                        .map(|server| ServerEntry {
                            country: server.country.clone().into(),
                            city: server.city.clone().into(),
                            load: server.load.clone().into(),
                            load_level: to_slint_load_level(&server.load_level),
                            enabled: server.enabled,
                        })
                        .collect::<Vec<_>>(),
                );

                ServerSection {
                    category: to_slint_category(&section.category),
                    helptext: section.helptext.clone().into(),
                    servers: ModelRc::new(server_model),
                }
            })
            .collect::<Vec<_>>(),
    );

    server_list.set_sections(ModelRc::new(section_model));
}

// start tray sync, to be called in main
pub fn start_sync(tray: &GephTray, service: Arc<UiService>) {
    // a root tray handle
    let weak_tray = tray.as_weak();

    // ConnInfo sync to Slint Tray
    let weak_tray_conn = weak_tray.clone();
    let mut conn_info_rx = service.subscribe_conn_info();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let conn_info = conn_info_rx.borrow().clone();
            let weak_tray = weak_tray_conn.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_conn_info(&tray, &conn_info);
                }
            });
        }
        // Future updates
        while conn_info_rx.changed().await.is_ok() {
            let conn_info = conn_info_rx.borrow_and_update().clone();
            let weak_tray = weak_tray_conn.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_conn_info(&tray, &conn_info);
                }
            });
        }
    })
    .detach();

    // Selected Server sync to Slint Tray
    let weak_tray_exit = weak_tray.clone();
    let mut exit_select_rx = service.subscribe_server_selection();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let server_selection = exit_select_rx.borrow().clone();
            let weak_tray = weak_tray_exit.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_exit_selection(&tray, &server_selection);
                }
            });
        }
        // Future updates
        while exit_select_rx.changed().await.is_ok() {

            // println!("Entered Exit Sync");
            let server_selection = exit_select_rx.borrow_and_update().clone();
            let weak_tray = weak_tray_exit.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_exit_selection(&tray, &server_selection);
                }
            });
        }
    })
    .detach();

    // Server list sync to Slint Tray
    let weak_tray_list = weak_tray.clone();
    let mut server_list_rx = service.subscribe_server_sections();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let server_list = server_list_rx.borrow().clone();
            let weak_tray = weak_tray_list.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_server_sections(&tray, &server_list);
                }
            });
        }
        // Future updates
        while server_list_rx.changed().await.is_ok() {
            let server_list = server_list_rx.borrow_and_update().clone();
            let weak_tray = weak_tray_list.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_server_sections(&tray, &server_list);
                }
            });
        }
    })
    .detach();

    // TunnelSettings sync to Slint
    let weak_tray_settings = weak_tray.clone();
    let mut settings_rx = service.subscribe_tunnel_settings();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let settings = settings_rx.borrow().clone();
            let weak_tray = weak_tray_settings.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_settings(&tray, &settings);
                }
            });
        }
        // Future updates
        while settings_rx.changed().await.is_ok() {
            let settings = settings_rx.borrow_and_update().clone();
            let weak_tray = weak_tray_settings.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_settings(&tray, &settings);
                }
            });
        }
    })
    .detach();

    // LoginState sync
    let weak_tray_login = weak_tray.clone();
    let mut login_rx = service.subscribe_login_state();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let state = login_rx.borrow().clone();
            let weak_tray = weak_tray_login.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_login_state(&tray, state);
                }
            });
        }

        // Future updates
        while login_rx.changed().await.is_ok() {
            let state = login_rx.borrow_and_update().clone();
            let weak_tray = weak_tray_login.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(tray) = weak_tray.upgrade() {
                    sync_login_state(&tray, state);
                }
            });
        }
    }).detach();
}
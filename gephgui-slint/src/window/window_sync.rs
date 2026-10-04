use::std::sync::Arc;

use crate::ui_states::{
    ServerLoadLevel,
    UiConnInfo,
    UiConnState,
    UiExitCategory,
    UiExitSelection,
    UiServerSections,
    UiTunnelSettings,
    UiUserInfo,
};

use crate::ui_service::UiService;

use crate::{GephWindow};
use crate::ConnectionState;
use crate::{
    ConnInfo,
    SelectedServer,
    LoadLevel,
    ServerEntry,
    ServerSection,
    ServerList,
    TunnelSettings,
    ProxySettings,
    LoginState,
    AccountStatus,
};

use slint::{ComponentHandle, SharedString, ModelRc, VecModel};

// Unify this enum later - using Slint definition only
pub fn to_slint_conn_state(state: &UiConnState) -> ConnectionState {
    match state {
        UiConnState::Disconnected => ConnectionState::Disconnected,
        UiConnState::Connecting => ConnectionState::Connecting,
        UiConnState::Connected => ConnectionState::Connected,
    }
}

pub fn to_slint_category(category: &UiExitCategory) -> SharedString {
    match category {
        UiExitCategory::Core => "Core".into(),
        UiExitCategory::Streaming => "Streaming".into(),
    }
}

// Unify this enum later - using Slint definition only
pub fn to_slint_load_level(level: &ServerLoadLevel) -> LoadLevel {
    match level {
        ServerLoadLevel::Low => LoadLevel::Low,
        ServerLoadLevel::Medium => LoadLevel::Medium,
        ServerLoadLevel::High => LoadLevel::High,
    }
}

fn sync_login_state(window: &GephWindow, state: LoginState) {
    window
    .global::<AccountStatus>()
    .set_login_status(state.into());
}

fn sync_conn_info(window: &GephWindow, info: &UiConnInfo) {

    let conn = window.global::<ConnInfo>();

    conn.set_conn_state(to_slint_conn_state(&info.state));
    conn.set_country(info.country.clone().into());
    conn.set_city(info.city.clone().into());
    conn.set_exit_ip(info.exit.clone().into());
    conn.set_bridge_ip(info.bridge.clone().into());
    conn.set_protocol(info.protocol.clone().into());
}

fn sync_exit_selection(window: &GephWindow, selection: &UiExitSelection) {

    let server = window.global::<SelectedServer>();
    // println!("[sync_exit_selection entered.");
    
    match selection {
        UiExitSelection::Auto => {
            server.set_country("Auto".into());
            server.set_city("".into());
            // println!("[Sync_to_Slint]: Auto");
        }
        UiExitSelection::Manual { country, city } => {
            server.set_country(country.clone().into());
            server.set_city(city.clone().into());
            // println!("[Sync_to_Slint]: Manual");
        }
    }
}

fn sync_server_sections(window: &GephWindow, sections: &UiServerSections) {
    let server_list = window.global::<ServerList>();

    let section_model = VecModel::from(
        sections
            .iter()
            .map(|section| {
                let server_model = VecModel::from(
                    section
                        .servers
                        .iter()
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

pub fn sync_settings(window: &GephWindow, settings: &UiTunnelSettings) {
    let tunnel_settings = window.global::<TunnelSettings>();
    let proxy_settings = window.global::<ProxySettings>();

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

pub fn sync_user_info(window: &GephWindow, user_info: &UiUserInfo) {
    let account_status = window.global::<AccountStatus>();

    account_status.set_account_level(user_info.account_level.clone().into());
    account_status.set_expire_date(user_info.expire_date.clone().into());
    account_status.set_remaining_days(user_info.remaining_days.clone().into());
    account_status.set_remaining_percentage(user_info.remaining_percent.clone());
    account_status.set_remaining_usage(user_info.remaining_usage.clone().into());
}

// start Slint sync
pub fn start_sync(window: &GephWindow, service: Arc<UiService>) {
    let weak_window = window.as_weak();

    // ConnInfo sync to Slint
    let weak_window_conn = weak_window.clone();
    let mut conn_info_rx = service.subscribe_conn_info();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let conn_info = conn_info_rx.borrow().clone();
            let weak_window = weak_window_conn.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_conn_info(&window, &conn_info);
                }
            });
        }
        // Future updates
        while conn_info_rx.changed().await.is_ok() {
            let conn_info = conn_info_rx.borrow_and_update().clone();
            let weak_window = weak_window_conn.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_conn_info(&window, &conn_info);
                }
            });
        }
    })
    .detach();

    // Selected Server sync to Slint
    let weak_window_exit = weak_window.clone();
    let mut exit_select_rx = service.subscribe_server_selection();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let server_selection = exit_select_rx.borrow().clone();
            let weak_window = weak_window_exit.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_exit_selection(&window, &server_selection);
                }
            });
        }
        // Future updates
        while exit_select_rx.changed().await.is_ok() {

            // println!("Entered Exit Sync");
            let server_selection = exit_select_rx.borrow_and_update().clone();
            let weak_window = weak_window_exit.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_exit_selection(&window, &server_selection);
                }
            });
        }
    })
    .detach();

    // Server list sync to Slint
    let weak_window_list = weak_window.clone();
    let mut server_list_rx = service.subscribe_server_sections();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let server_list = server_list_rx.borrow().clone();
            let weak_window = weak_window_list.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_server_sections(&window, &server_list);
                }
            });
        }
        // Future updates
        while server_list_rx.changed().await.is_ok() {
            let server_list = server_list_rx.borrow_and_update().clone();
            let weak_window = weak_window_list.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_server_sections(&window, &server_list);
                }
            });
        }
    })
    .detach();

    // TunnelSettings sync to Slint
    let weak_window_settings = weak_window.clone();
    let mut settings_rx = service.subscribe_tunnel_settings();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let settings = settings_rx.borrow().clone();
            let weak_window = weak_window_settings.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_settings(&window, &settings);
                }
            });
        }
        
        // Future updates
        while settings_rx.changed().await.is_ok() {
            let settings = settings_rx.borrow_and_update().clone();
            let weak_window = weak_window_settings.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_settings(&window, &settings);
                }
            });
        }
    }).detach();

    // LoginState sync
    let weak_window_login = weak_window.clone();
    let mut login_rx = service.subscribe_login_state();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let state = login_rx.borrow().clone();
            let weak_window = weak_window_login.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_login_state(&window, state);
                }
            });
        }

        // Future updates
        while login_rx.changed().await.is_ok() {
            let state = login_rx.borrow_and_update().clone();
            let weak_window = weak_window_login.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_login_state(&window, state);
                }
            });
        }
    }).detach();

    // AccountStatus sync
    let weak_window_user_info = weak_window.clone();
    let mut user_info_rx = service.subscribe_user_info();

    geph5_rt::spawn(async move {
        // Initial sync
        {
            let info = user_info_rx.borrow().clone();
            let weak_window = weak_window_user_info.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_user_info(&window, &info);
                }
            });
        }

        // Future updates
        while user_info_rx.changed().await.is_ok() {
            let info = user_info_rx.borrow_and_update().clone();
            let weak_window = weak_window_user_info.clone();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_window.upgrade() {
                    sync_user_info(&window, &info);
                }
            });
        }
    }).detach();

}
use std::{
    sync::Arc,
    time::Duration,
};
use anyhow::Ok;

use tokio::sync::{watch, RwLock};

use crate::LoginState;
use crate::manager;

use geph5_misc_rpc::manager_control::{TunnelSettings};

use crate::ui_states::{
    RpcRawStates,
    UiUserInfo,
    UiConnInfo,
    UiServerSections,
    UiExitSelection,
    UiTunnelSettings,
    UiConnState
};

const NORMAL_INTERVAL: Duration = Duration::from_secs(1);
const BURST_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub struct UiService {
    // User account secret cache
    secret: Arc<RwLock<Option<String>>>,

    // login_state and watch channel
    login_state: Arc<RwLock<LoginState>>,   
    login_state_tx: watch::Sender<LoginState>,

    // Latest raw state from backend/RPC.
    raw_state: Arc<tokio::sync::RwLock<RpcRawStates>>,

    // UI state channels.
    user_info_tx: watch::Sender<UiUserInfo>,
    conn_info_tx: watch::Sender<UiConnInfo>,
    tunnel_settings_tx: watch::Sender<UiTunnelSettings>,
    server_sections_tx: watch::Sender<UiServerSections>,
    server_selection_tx: watch::Sender<UiExitSelection>,

    // UI polling refresh channels
    // login_state_refresh_tx: watch::Sender<u64>,
    conn_info_refresh_tx: watch::Sender<u64>,
    tunnel_settings_refresh_tx: watch::Sender<u64>,
    net_status_refresh_tx: watch::Sender<u64>,
    user_info_refresh_tx: watch::Sender<u64>,
}

impl UiService {

    // UiService initialize
    pub fn new(secret: Option<String>) -> Self {
        let login_state = if secret.is_some() {
            LoginState::Authenticating
        } else {
            LoginState::LoggedOut
        };

        let (login_state_tx, _) =
            watch::channel(login_state);

        let (user_info_tx, _) =
            watch::channel(UiUserInfo::default());

        let (conn_info_tx, _) =
            watch::channel(UiConnInfo::default());

        let (tunnel_settings_tx, _) =
            watch::channel(UiTunnelSettings::default());

        let (server_sections_tx, _) =
            watch::channel(Vec::new());

        let (server_selection_tx, _) =
            watch::channel(UiExitSelection::Auto);

        // refresh channels...
        let (conn_info_refresh_tx, _) = watch::channel(0u64);
        let (tunnel_settings_refresh_tx, _) = watch::channel(0u64);
        let (net_status_refresh_tx, _) = watch::channel(0u64);
        let (user_info_refresh_tx, _) = watch::channel(0u64);

        Self {
            secret: Arc::new(RwLock::new(secret)),
            login_state: Arc::new(RwLock::new(login_state)),
            login_state_tx,

            raw_state: Arc::new(RwLock::new(RpcRawStates::default())),

            user_info_tx,
            conn_info_tx,
            tunnel_settings_tx,
            server_sections_tx,
            server_selection_tx,

            // refresh channels...
            user_info_refresh_tx,
            conn_info_refresh_tx,
            tunnel_settings_refresh_tx,
            net_status_refresh_tx,
        }
    }

    // Updater of each RpcRawState
    async fn update_conn_info(&self) -> anyhow::Result<()> {
        let conn_info = manager::fetch_conn_info().await?;
        // println!("[UiRpc] RawConnInfo: {conn_info:?}");

        let raw_state = {
            let mut state = self.raw_state.write().await;
            state.conn_info = conn_info;
            state.clone()
        };

        let ui_conn_info = raw_state.to_ui_conn_info();
        // println!("[UiRpc] UiConnInfo: {ui_conn_info:?}");

        let _ = self.conn_info_tx.send(ui_conn_info);
        Ok(())
    }

    async fn update_user_info(&self) -> anyhow::Result<()> {
        let user_secret = self.secret.read().await.clone();
        if let Some(secret) = user_secret {
            let user_info = manager::fetch_user_info(secret).await?;
            // println!("[UiRpc] RawUserInfo: {user_info:?}");

            let raw_state = {
                let mut state = self.raw_state.write().await;
                state.user_info = user_info;
                state.clone()
            };

            let ui_user_info = raw_state.to_ui_user_info();

            // println!("[UiRpc] UiUserInfo: {ui_user_info:?}");

            let _ = self.user_info_tx.send(ui_user_info);
            self.set_login_state(LoginState::LoggedIn).await;

        } else {
            // Secret is None and set UI State to LoggedOut
            // self.set_login_state(LoginState::LoggedOut);
        }

        Ok(())
    }

    async fn update_tunnel_settings(&self) -> anyhow::Result<()> {
        let tunnel_settings = manager::fetch_tunnel_settings().await?;
        // println!("[UiRpc] Raw TunnelSettings: {tunnel_settings:?}");

        let raw_state = {
            let mut state = self.raw_state.write().await;
            state.tunnel_settings = tunnel_settings;
            state.clone()
        };

        let ui_tunnel_settings = raw_state.to_ui_tunnel_settings();
        let ui_server_selection = raw_state.to_ui_server_selection();

        // println!("[UiRpc] UiTunnelSettings: {ui_tunnel_settings:?}");
        // println!("[UiRpc] UiServerSelection: {ui_server_selection:?}");

        let _ = self.tunnel_settings_tx.send(ui_tunnel_settings);
        let _ = self.server_selection_tx.send(ui_server_selection);

        Ok(())
    }

    async fn update_net_status(&self) -> anyhow::Result<()> {
        let net_status = manager::fetch_net_status().await?;

        // println!("[UiRpc] RawNetStatus: {net_status:?}");

        let raw_state = {
            let mut state = self.raw_state.write().await;
            state.net_status = net_status;
            state.clone()
        };

        let ui_user_info = raw_state.to_ui_user_info();

        let ui_server_sections =
            raw_state.to_ui_server_sections(
                &ui_user_info.account_level
            );

        // println!("[UiRpc] UiServerSections: {ui_server_sections:?}");

        let _ = self.server_sections_tx.send(ui_server_sections);

        Ok(())
    }

    // RPC state polling refresh triggers
    // Call these for an immediate state poll and UI refresh
    pub fn poll_conn_info(&self) {
        let _ = self.conn_info_refresh_tx.send_modify(|v| {
            *v += 1;
        });
    }

    pub fn poll_tunnel_settings(&self) {
        let _ = self.tunnel_settings_refresh_tx.send_modify(|v| {
            *v += 1;
        });
    }

    pub fn poll_net_status(&self) {
        let _ = self.net_status_refresh_tx.send_modify(|v| {
            *v += 1;
        });
    }

    pub fn poll_user_info(&self) {
        let _ = self.user_info_refresh_tx.send_modify(|v| {
            *v += 1;
        });
    }

    pub fn poll_all(&self) {
        self.poll_conn_info();
        self.poll_tunnel_settings();
        self.poll_net_status();
        self.poll_user_info();
    }

    // run polling for ConnInfo RPC request
    async fn run_conn_info_poller(&self) {
        let mut interval = tokio::time::interval(NORMAL_INTERVAL);

        let mut refresh_rx = self.conn_info_refresh_tx.subscribe();

        let mut burst = false;
        let mut burst_target = UiConnState::Disconnected;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) =
                        self.update_conn_info().await
                    {
                        println!("[Poller] ConnInfo error: {e:?}");
                        continue;
                    }

                    // check if reached target ConnState and return to normal polling
                    if burst {
                        let conn_info = self.current_conn_info().await;

                        if conn_info.state == burst_target {
                            println!("[Poller] ConnInfo: burst finished");

                            burst = false;

                            interval = tokio::time::interval(NORMAL_INTERVAL);
                        }
                    } 
                }

                // When poll_conn_info() called (Connect button clicked)
                // enter burst mode for fast UI repsonse
                result = refresh_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    println!("[Poller] ConnInfo: enter burst");
                    let origin_conn_info = self.current_conn_info().await;
                    match origin_conn_info.state {
                        UiConnState::Disconnected => {
                            burst_target = UiConnState::Connected;
                        }
                        UiConnState::Connecting => {
                            burst_target = UiConnState::Disconnected;
                        }
                        UiConnState::Connected => {
                            burst_target = UiConnState::Disconnected;
                        }
                    }

                    burst = true;
                    interval = tokio::time::interval(BURST_INTERVAL);
                }
            }
        }
    }

    // run polling for TunnelSettings RPC request
    async fn run_tunnel_settings_poller(&self) {
        let mut interval =
            tokio::time::interval(NORMAL_INTERVAL);

        let mut refresh_rx =
            self.tunnel_settings_refresh_tx.subscribe();

        let mut burst = false;
        let mut last_settings: Option<TunnelSettings> = None;
        let mut stable_count = 0u8;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // println!("[Poller] TunnelSettings: normal refresh");

                    if let Err(e) =
                        self.update_tunnel_settings().await
                    {
                        println!("[Poller] TunnelSettings error: {e:?}");
                    }

                    // check if reached target setting and stop burst
                    if burst {
                        let new_settings = self.current_tunnel_settings().await;

                        if last_settings.as_ref() == Some(&new_settings) {
                            stable_count += 1;
                        } else {
                            stable_count = 0;
                            last_settings = Some(new_settings);
                        }

                        // finish burst if Settings remained unchanged for 2 polls
                        if stable_count > 1 {
                            println!(
                                "[Poller] TunnelSettings: burst finished"
                            );

                            burst = false;
                            last_settings = None;
                            stable_count = 0;

                            interval =
                                tokio::time::interval(NORMAL_INTERVAL);
                        }
                    }
                }

                result = refresh_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    println!("[Poller] TunnelSettings: enter burst");

                    last_settings = Some(self.current_tunnel_settings().await);

                    stable_count = 0;
                    burst = true;
                    interval = tokio::time::interval(BURST_INTERVAL);
                }
            }
        }
    }

    // run polling for NetStatus RPC request
    async fn run_net_status_poller(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        let mut login_rx = self.login_state_tx.subscribe();
        let mut polling = *login_rx.borrow() != LoginState::LoggedOut;

        let mut refresh_rx = self.net_status_refresh_tx.subscribe();

        loop {
            tokio::select! {
                _ = interval.tick(), if polling => {
                    // println!("[Poller] NetStatus: normal refresh");

                    if let Err(e) =
                        self.update_net_status().await
                    {
                        println!("[Poller] NetStatus error: {e:?}");
                    }
                }

                // Immdiate refresh if triggered by calling poll_net_status()
                result = refresh_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    println!("[Poller] NetStatus: immediate refresh");

                    if let Err(e) =
                        self.update_net_status().await
                    {
                        println!("[Poller] NetStatus error: {e:?}");
                    }
                }
                // Pause polling when logged out and resume if secret exists
                result = login_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    let state = *login_rx.borrow();
                    match state {
                        LoginState::LoggedOut => {
                            // println!("[Poller] NetStatus: paused (LoggedOut)");
                            polling = false;
                        }
                        _ => {
                            // println!("[Poller] NetStatus: resumed");
                            polling= true;
                        }
                    }
                }
            }
        }
    }

    // run polling for UserInfo RPC request
    async fn run_user_info_poller(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));

        let mut refresh_rx = self.user_info_refresh_tx.subscribe();
        let mut login_rx = self.login_state_tx.subscribe();

        let mut polling = *login_rx.borrow() != LoginState::LoggedOut;

        loop {
            tokio::select! {
                _ = interval.tick(), if polling => {
                    // println!("[Poller] UserInfo: normal refresh");

                    if let Err(e) =
                        self.update_user_info().await
                    {
                        println!("[Poller] UserInfo error: {e:?}");
                    }
                    // poll NetStatus right after account_level updated for ServerList refresh
                    self.poll_net_status();
                }

                // Immdiate refresh if triggered by calling poll_user_info()
                result = refresh_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    println!("[Poller] UserInfo: immediate refresh");

                    if let Err(e) =
                        self.update_user_info().await
                    {
                        println!("[Poller] UserInfo error: {e:?}");
                    }
                }

                // Pause polling when logged out and resume if secret exists
                result = login_rx.changed() => {
                    if result.is_err() {
                        break;
                    }

                    let state = *login_rx.borrow();
                    match state {
                        LoginState::LoggedOut => {
                            // println!("[Poller] UserInfo: paused (LoggedOut)");
                            polling = false;
                        }
                        _ => {
                            // println!("[Poller] UserInfo: resumed");
                            polling= true;
                        }
                    }
                }
            }
        }
    }

    // state change subcribers
    pub fn subscribe_user_info(&self) -> watch::Receiver<UiUserInfo> {
        self.user_info_tx.subscribe()
    }

    pub fn subscribe_conn_info(&self) -> watch::Receiver<UiConnInfo> {
        self.conn_info_tx.subscribe()
    }

    pub fn subscribe_tunnel_settings(&self) -> watch::Receiver<UiTunnelSettings> {
        self.tunnel_settings_tx.subscribe()
    }

    pub fn subscribe_server_sections(&self) -> watch::Receiver<UiServerSections> {
        self.server_sections_tx.subscribe()
    }

    pub fn subscribe_server_selection(&self) -> watch::Receiver<UiExitSelection> {
        self.server_selection_tx.subscribe()
    }

    pub fn subscribe_login_state(&self) -> watch::Receiver<LoginState> {
        self.login_state_tx.subscribe()
    }

    // RPC State polling starter
    pub fn start_polling(self: Arc<Self>) {
        geph5_rt::spawn({
            let service = Arc::clone(&self);

            async move {
                service.run_conn_info_poller().await;
            }
        }).detach();

        geph5_rt::spawn({
            let service = Arc::clone(&self);

            async move {
                service.run_tunnel_settings_poller().await;
            }
        }).detach();

        geph5_rt::spawn({
            let service = Arc::clone(&self);

            async move {
                service.run_net_status_poller().await;
            }
        }).detach();

        geph5_rt::spawn({
            let service = Arc::clone(&self);

            async move {
                service.run_user_info_poller().await;
            }
        }).detach();
    }

    // Get current states for Slint callbacks
    // Use UIConnInfo for Slint callbacks since its enum is simple
    pub async fn current_conn_info(&self) -> UiConnInfo {
        self.raw_state
            .read().await
            .to_ui_conn_info()
    }

    pub async fn current_tunnel_settings(&self) -> TunnelSettings {
        self.raw_state
            .read().await
            .tunnel_settings
            .clone()
    }

    pub async fn current_secret(&self) -> String {
        self.secret
            .read()
            .await
            .clone()
            .expect("current_secret() called with null secret")
    }

    pub async fn set_login_state(&self, target_state: LoginState) {
        // println!("[UiService] LoginState -> {:?}", target_state);
        *self.login_state.write().await = target_state;
        let _ = self.login_state_tx.send(target_state);
    }

    pub async fn set_secret(&self, secret: String) {
        *self.secret.write().await = Some(secret);
    }

    pub async fn clear_secret(&self) {
        *self.secret.write().await = None;
    }

    pub async fn clear_service(&self) {
        *self.raw_state.write().await = RpcRawStates::default();
    }
}
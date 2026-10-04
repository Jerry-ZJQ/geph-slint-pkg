//! Talks to the privileged `geph5 manager` (the geph5-app supervisor) over
//! its control protocol, instead of spawning geph5-client ourselves.
//!
//! The manager owns the engine lifecycle: it always keeps a child geph5-client
//! running (a dry-run instance while disconnected, a real tunnel while
//! connected), so engine/broker queries forwarded through `daemon_rpc` work
//! whether or not we're connected. We only translate the GUI's lifecycle calls
//! (`start_daemon` / `stop_daemon` / `restart_daemon`) into the manager's
//! `GephCtl` methods, and forward everything else through `daemon_rpc`.
//!
//! Both the protocol (`GephCtlProtocol`) and the transport dialing the
//! manager's control endpoint (unix socket / Windows named pipe) come from
//! `geph5_misc_rpc::manager_control` — the same definitions the manager and the
//! `geph` CLI compile against, so the endpoint and the wire types cannot drift.

use std::{future::Future, sync::LazyLock};

use geph5_broker_protocol::{ExitConstraint, Credential, UserInfo, NetStatus};
use geph5_misc_rpc::manager_control::{
    self, GephCtlClient, GephCtlError, SessionContext, TunnelSettings,
};
use geph5_misc_rpc::client_control::{ConnInfo};
use isocountry::CountryCode;
use nanorpc::{JrpcRequest, JrpcResponse, RpcTransport, JrpcId};
use serde_json::{Value, json};
use serde::de::{DeserializeOwned};
use serde::Deserialize;

use crate::ui_states::UiExitSelection;

// #[derive(Clone, Eq, PartialEq)]
// pub struct DaemonArgs {
//     pub secret: String,
//     pub metadata: serde_json::Value,
//     pub prc_whitelist: bool,
//     pub exit: UiExitSelection,
//     pub global_vpn: bool,
//     pub proxy: Option<ProxySettings>,
//     pub allow_lan: bool,
//     pub allow_direct: bool,
// }

/// The shared typed client pointed at the running manager. Each call dials a
/// fresh connection (the transport has no pooling); this just avoids rebuilding
/// the wrapper.
fn client() -> &'static GephCtlClient {
    static CLIENT: LazyLock<GephCtlClient> = LazyLock::new(manager_control::manager_control_client);
    &CLIENT
}

/// Await a `GephCtl` call, flattening the transport and application error
/// layers into one `anyhow` error.
async fn ctl<T>(
    fut: impl Future<Output = Result<Result<T, String>, GephCtlError<anyhow::Error>>>,
) -> anyhow::Result<T> {
    match fut.await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(msg)) => Err(anyhow::anyhow!(msg)),
        Err(e) => Err(anyhow::anyhow!("could not reach the geph manager: {e:?}")),
    }
}

/// The calling desktop session, so the (root) manager configures *our* proxy.
/// This is just identity — uid plus a few env vars; the proxy logic is the
/// manager's.
fn session() -> SessionContext {
    #[cfg(unix)]
    {
        SessionContext {
            uid: unsafe { libc::geteuid() },
            gid: Some(unsafe { libc::getegid() }),
            home: std::env::var("HOME").ok(),
            dbus_session_bus_address: std::env::var("DBUS_SESSION_BUS_ADDRESS").ok(),
            xdg_runtime_dir: std::env::var("XDG_RUNTIME_DIR").ok(),
        }
    }
    #[cfg(not(unix))]
    {
        SessionContext::default()
    }
}

/// Translate the GUI's exit selection into a geph `ExitConstraint`.
pub fn exit_constraint(exit: &UiExitSelection) -> anyhow::Result<ExitConstraint> {
    Ok(match exit {
        UiExitSelection::Auto => ExitConstraint::Auto,
        UiExitSelection::Manual { city, country } => ExitConstraint::CountryCity(
            CountryCode::for_alpha2(country)
                .map_err(|_| anyhow::anyhow!("bad country code {country}"))?,
            city.clone(),
        ),
    })
}

// fn tunnel_settings(args: &DaemonArgs) -> anyhow::Result<TunnelSettings> {
//     Ok(TunnelSettings {
//         exit_constraint: exit_constraint(&args.exit)?,
//         proxy: args.proxy.clone(),
//         vpn: args.global_vpn,
//         allow_lan: args.allow_lan,
//         allow_direct: args.allow_direct,
//         passthrough_china: args.prc_whitelist,
//         session_metadata: args.metadata.clone(),
//     })
// }

// Send Secret to Daemon as part of user login
pub async fn secret_to_daemon(secret: String) -> anyhow::Result<()> {
    ctl(client().set_secret(secret.clone())).await?;
    Ok(())
}


/// Get and set tunnel settings for UI access and control.
pub async fn get_tunnel_settings() -> anyhow::Result<TunnelSettings> {
    let view = ctl(client().get_settings()).await?;
    Ok(view.tunnel_settings())
}

pub async fn apply_tunnel_settings(settings: TunnelSettings) -> anyhow::Result<()> {
    ctl(client().apply_settings(settings, session())).await?;
    Ok(())
}

// pub async fn start_daemon(args: DaemonArgs) -> anyhow::Result<()> {
//     // Hand the manager the secret WITHOUT re-validating it against the broker.
//     // The GUI already validated the secret at its login screen, so a broker
//     // round-trip here would only re-check something known-good while blocking the
//     // connect path on a slow or dead network. `set_secret` is purely local; the
//     // tunnel engine authenticates the secret itself as it connects, and a bad
//     // secret surfaces as a normal connection failure.
//     ctl(client().set_secret(args.secret.clone())).await?;
//     ctl(client().apply_settings(tunnel_settings(&args)?, session())).await?;
//     ctl(client().connect(session())).await?;
//     Ok(())
// }

// Replacing the offical start_daemon with separated secret and TunnelSettings as args
pub async fn start_daemon(secret: String, settings: TunnelSettings) -> anyhow::Result<()> {
    ctl(client().set_secret(secret.clone())).await?;
    ctl(client().apply_settings(settings, session())).await?;
    ctl(client().connect(session())).await?;
    Ok(())
}

pub async fn restart_daemon(settings: TunnelSettings) -> anyhow::Result<()> {
    // removed set_secret from geph official version.
    // stop daemon, apply settings and then start daemon for a clean re-connect 
    ctl(client().disconnect(session())).await?;
    ctl(client().apply_settings(settings, session())).await?;
    ctl(client().connect(session())).await?;
    Ok(())
}

pub async fn stop_daemon() -> anyhow::Result<()> {
    ctl(client().disconnect(session())).await?;
    Ok(())
}

/// Disconnect and forget the manager's persisted account secret.
/// also restores settings to default
pub async fn logout() -> anyhow::Result<()> {
    ctl(client().disconnect(session())).await?;

    let settings = TunnelSettings {
        exit_constraint: ExitConstraint::Auto,
        vpn: true,
        allow_lan: false,
        allow_direct: false,
        passthrough_china: false,
        session_metadata: Value::Null,
        proxy: None,
    };
    ctl(client().apply_settings(settings, session())).await?;

    ctl(client().logout(session())).await?;
    Ok(())
}

/// Reconnect using the manager's already-persisted secret + exit constraint, with
/// no `DaemonArgs` from the JS UI. This is what the tray "Connect" action uses:
/// the manager keeps the last-used settings, so a bare `connect` brings the tunnel
/// back up exactly as the user last had it.
pub async fn reconnect() -> anyhow::Result<()> {
    ctl(client().connect(session())).await?;
    Ok(())
}

/// Switch the exit constraint. The manager persists it and, if currently
/// connected, reconnects to the new exit WITHOUT a leak window (the kill switch
/// stays up; only the engine child is restarted).
/// changed official return type from () to TunnelSettings
pub async fn set_exit_constraint(exit: &UiExitSelection) -> anyhow::Result<TunnelSettings> {
    let view = ctl(client().get_settings()).await?;
    let mut settings = view.tunnel_settings();
    settings.exit_constraint = exit_constraint(exit)?;
    ctl(client().apply_settings(settings.clone(), session())).await?;
    Ok(settings)
}

/// Whether the manager's control endpoint is up and answering at all (regardless
/// of connection state). The raw `ping` call is intentionally lock-free in the
/// manager and never reaches an engine or the network. We do not turn elapsed
/// time into a false "dead" result: only a definite transport/RPC failure does.
#[cfg(any(unix, windows))]
pub async fn manager_reachable() -> bool {
    let req = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "ping".into(),
        params: vec![],
        id: nanorpc::JrpcId::Number(0),
    };
    match manager_control::manager_control_transport()
        .call_raw(req)
        .await
    {
        Ok(resp) => resp.error.is_none(),
        Err(_) => false,
    }
}

/// Whether the user currently wants the tunnel up (mirrors the old "is the
/// manager process running" semantics, which only existed while connected).
pub async fn manager_connected() -> bool {
    match client().get_settings().await {
        Ok(Ok(settings)) => settings.connected,
        _ => false,
    }
}

/// Forward a raw engine RPC (`conn_info`, `broker_rpc`, `net_status`,
/// `stat_history`, `recent_logs`, `start_registration`, …) to the manager, which
/// relays it to its always-running child geph5-client. This is what makes
/// broker/engine calls work whether or not we're connected.
pub async fn daemon_rpc(inner: JrpcRequest) -> anyhow::Result<JrpcResponse> {
    let req = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "daemon_rpc".into(),
        params: vec![json!(inner.method), Value::Array(inner.params)],
        id: inner.id.clone(),
    };
    let mut resp = manager_control::manager_control_transport()
        .call_raw(req)
        .await?;
    // The manager's `daemon_rpc` result/error already reflects the inner call.
    resp.id = inner.id;
    Ok(resp)
}

// common daemon_rpc call and json response deserialization
pub async fn call_daemon_rpc<T>(request: JrpcRequest) -> anyhow::Result<T> where T: DeserializeOwned {
    let response = daemon_rpc(request).await?;

    if let Some(error) = response.error {
        anyhow::bail!("daemon RPC error: {:?}", error);
    }

    let result = response
        .result
        .ok_or_else(|| anyhow::anyhow!("daemon RPC returned no result"))?;

    // let value = serde_json::from_value::<T>(result)?;
    // Ok(value)
    serde_json::from_value::<T>(result)
        .map_err(|e| anyhow::anyhow!("Invalid daemon RPC response: {e}"))
}

// fetch ConnInfo from RPC and deserialize for RpcRawStates cache 
pub async fn fetch_conn_info() -> anyhow::Result<ConnInfo> {
    let request = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "conn_info".into(),
        params: vec![],
        id: JrpcId::String("req_conn_info".into()),
    };

    let resp = call_daemon_rpc(request).await?;
    Ok(resp)
}

// fetch net_status from RPC and deserialize for RpcRawStates cache 
pub async fn fetch_net_status() -> anyhow::Result<NetStatus> {
    let request = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "net_status".into(),
        params: vec![],
        id: JrpcId::String("req_net_status".into()),
    };

    let resp = call_daemon_rpc(request).await?;
    Ok(resp)
}

// fetch tunnel_settings for RpcRawStates cache, simple wrapper of get_tunnel_settings()
pub async fn fetch_tunnel_settings() -> anyhow::Result<TunnelSettings> {
    get_tunnel_settings().await
}

// fetch UserInfo from RPC and deserialize for RpcRawStates cache 
pub async fn fetch_user_info(secret: String) -> anyhow::Result<UserInfo> {
    let request = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "broker_rpc".into(),
        params: vec![
            json!("get_user_info_by_cred"),
            json!([serde_json::to_value(Credential::Secret(secret))?]),
        ],
        id: JrpcId::String("req_user_info".into()),
    };

    let resp = call_daemon_rpc(request).await?;
    println!("[RPC resp]: {resp:?}");
    Ok(resp)
}

// Data model for auth_secret() broker_rpc response
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AccountSecretStatus {
    Retired(String),
    Invalid(String),
    Current {
        current: AccountSecretCurrent,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountSecretCurrent {
    pub invite_code: Option<String>,
    pub user_id: u64,
}

// Check secret status via broker_rpc
pub async fn auth_secret(secret: String) -> anyhow::Result<AccountSecretStatus> {
    let request = JrpcRequest {
        jsonrpc: "2.0".into(),
        method: "broker_rpc".into(),
        params: vec![
            json!("get_account_secret_status"),
            json!([secret]),
        ],
        id: JrpcId::String("req_secret_authentication".into()),
    };

    let resp = call_daemon_rpc(request).await?;
    println!("[RPC resp]: {resp:?}");
    Ok(resp)
}
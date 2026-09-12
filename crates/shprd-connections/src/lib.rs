//! Native profile and connection lifecycle contracts.
mod create;
mod error;
mod home_probe;
mod http;
mod lifecycle;
mod manager;
mod paths;
mod profile;
mod retry;
mod routing;
mod runtime;
mod service;
mod ssh;
mod store;
mod tunnel;
mod update;
pub use error::{Error, Result};
pub use http::{HttpEndpoint, HttpRoute, parse_http_route, raw_pathname, response_headers};
pub use manager::Manager;
pub use profile::{ConnectionId, LEGACY_ID, Profile, Registry, Transport};
pub use retry::{RetryPolicy, RetryTicket};
pub use routing::{
    ReplyPublisher, RpcRoute, envelope, event_envelope, query_generation, resolve_rpc,
};
pub use runtime::{
    Lease, Runtime, RuntimeContext, RuntimeFactory, RuntimeFuture, SocketPaths, State, Status,
    sanitize_error,
};
pub use service::{ProbeResult, ProfileFactory, ProfileService};
pub use ssh::{
    SshFailure, SshFailureKind, classify_ssh_failure, ssh_command_argv, ssh_tunnel_argv,
};
pub use store::Store;
pub use tunnel::SshTunnel;
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_reserved_profile_id() {
        let raw = r#"{"id":"legacy-default","label":"Local","type":"local","control_socket_path":"/tmp/control","client_socket_path":"/tmp/render","auto_connect":true}"#;
        assert!(super::Profile::parse(raw).is_err());
    }
}

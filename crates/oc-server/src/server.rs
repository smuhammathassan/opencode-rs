//! HTTP server listener. From reference/packages/opencode/src/server/server.ts.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use url::Url;

use crate::auth::AuthConfig;
use crate::cors::CorsOptions;
use crate::location::Location;
use oc_provider::auth::{AuthStore, Info};
use oc_util::util::signal::{process_shutdown, Signal};

/// Options for `listen`. Mirrors `ListenOptions` in
/// reference/packages/opencode/src/server/server.ts.
#[derive(Debug, Clone)]
pub struct ListenOptions {
    pub hostname: String,
    pub port: u16,
    pub cors: CorsOptions,
    pub auth: AuthConfig,
    pub mdns: bool,
    pub mdns_domain: Option<String>,
    /// Optional injected shutdown signal. Production callers use the
    /// process-wide signal; tests and embedders can provide their own.
    pub shutdown: Option<Arc<Signal>>,
}

impl ListenOptions {
    pub fn new(hostname: impl Into<String>, port: u16) -> Self {
        ListenOptions {
            hostname: hostname.into(),
            port,
            cors: CorsOptions::default(),
            auth: AuthConfig::from_env(),
            mdns: false,
            mdns_domain: None,
            shutdown: None,
        }
    }

    pub fn with_shutdown(mut self, shutdown: Arc<Signal>) -> Self {
        self.shutdown = Some(shutdown);
        self
    }
}

/// A running server. Mirrors `Listener` in reference/packages/opencode/src/server/server.ts.
#[derive(Debug)]
pub struct Listener {
    pub hostname: String,
    pub port: u16,
    pub url: Url,
    shutdown: tokio::sync::oneshot::Sender<()>,
    watcher_cancel: tokio_util::sync::CancellationToken,
    mdns: Option<crate::mdns::Advertisement>,
    handle: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Listener {
    /// Stop the listener; `force` closes active connections like `listener.stop(true)`.
    pub async fn stop(self, force: bool) {
        let _ = force;
        if let Some(mdns) = self.mdns {
            mdns.unpublish();
        }
        self.watcher_cancel.cancel();
        let _ = self.shutdown.send(());
        let _ = self.handle.await;
    }
}

/// Start the server. Port `0` prefers 4096 first, then any free port, matching
/// reference/packages/opencode/src/server/server.ts (`startWithPortFallback`).
pub async fn listen(opts: ListenOptions) -> std::io::Result<Listener> {
    if opts.port != 0 {
        return start_listener(opts).await;
    }
    let preferred = ListenOptions {
        port: 4096,
        ..opts.clone()
    };
    match start_listener(preferred).await {
        Ok(listener) => Ok(listener),
        Err(_) => start_listener(opts).await,
    }
}

async fn start_listener(opts: ListenOptions) -> std::io::Result<Listener> {
    let address = SocketAddr::new(
        opts.hostname.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid hostname")
        })?,
        opts.port,
    );
    let tcp = TcpListener::bind(address).await?;
    let port = tcp.local_addr()?.port();

    let location = Location::default_location();
    let config = load_config(&location).await;
    let database = oc_database::Database::open(oc_database::database::path()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("failed to open opencode database: {error}"),
        )
    })?;
    let mut state = crate::state::AppState::with_database_and_config(
        opts.auth,
        opts.cors,
        location.clone(),
        Arc::new(database),
        config.clone(),
    )
    .map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("failed to hydrate opencode database: {error}"),
        )
    })?;
    bootstrap_plugins(&mut state, &config);
    let watcher_cancel = tokio_util::sync::CancellationToken::new();
    start_config_watcher(&state, &location, watcher_cancel.clone());
    crate::projectors::init_projectors(state.clone());
    // MCP configuration is discovered from the active project and connected
    // asynchronously so a slow/unavailable remote server does not prevent the
    // HTTP listener from becoming usable. Connection failures remain visible
    // through the MCP status endpoint.
    tokio::spawn(crate::instance_handlers::auto_connect_mcps(state.clone()));
    let router = crate::router::build(state);

    let mdns = if opts.mdns {
        let publish =
            opts.hostname != "127.0.0.1" && opts.hostname != "localhost" && opts.hostname != "::1";
        if publish {
            let advertisement = crate::mdns::publish(port, opts.mdns_domain.as_deref());
            if advertisement.is_none() {
                tracing::warn!(
                    port,
                    "mDNS enabled but service advertisement is unavailable; continuing without discovery"
                );
            }
            advertisement
        } else {
            tracing::warn!("mDNS enabled but hostname is loopback; skipping mDNS publish");
            None
        }
    } else {
        None
    };

    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let process_shutdown = opts.shutdown.unwrap_or_else(process_shutdown);
    let handle = tokio::spawn(async move {
        axum::serve(tcp, router)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = receiver => {}
                    _ = process_shutdown.wait() => {}
                }
            })
            .await
    });

    let hostname = opts.hostname.clone();
    let url = {
        let mut url = Url::parse("http://localhost").unwrap();
        url.set_host(Some(&hostname)).ok();
        url.set_port(Some(port)).ok();
        url
    };

    // Keep the serve task from being dropped when `handle` is abandoned.
    let _ = handle;
    let handle = handle;

    Ok(Listener {
        hostname,
        port,
        url,
        shutdown,
        watcher_cancel,
        mdns,
        handle,
    })
}

/// Load local configured plugins before the router is exposed. The manager
/// owns QuickJS on its own thread, so a plugin cannot move across Tokio worker
/// threads. Pure mode intentionally skips all external plugin execution.
fn bootstrap_plugins(state: &mut crate::state::AppState, config: &serde_json::Value) {
    // `--pure` only suppresses configured/external plugins in the reference;
    // internal defaults are a separate bootstrap phase. The Rust port does
    // not yet ship those internal auth plugins, but keeping the phases
    // distinct prevents pure mode from becoming incorrect when they land.
    let pure = truthy_env_flag("OPENCODE_PURE");
    let internal_defaults = if truthy_env_flag("OPENCODE_DISABLE_DEFAULT_PLUGINS") {
        BTreeMap::new()
    } else {
        crate::builtin_auth::default_auth_hooks()
    };
    let declarations = if pure {
        Vec::new()
    } else {
        configured_plugin_declarations(config)
    };
    if declarations.is_empty() {
        state.provider_auth = Arc::new(crate::plugin_auth::from_builtins(internal_defaults));
        return;
    }

    let stores = Arc::clone(&state.stores);
    let host = oc_plugin::LocalHost::with_registration_sink(state.plugin_registrations.clone())
        .with_client_rpc(move |request| {
            if request.method != "session.status" {
                return Err(format!(
                    "client.{} is not implemented by the server host",
                    request.method
                ));
            }
            let stores = stores
                .try_read()
                .map_err(|_| "client.session.status snapshot is busy".to_string())?;
            let mut status = serde_json::Map::new();
            for (id, record) in &stores.sessions {
                status.insert(
                    id.clone(),
                    serde_json::json!({
                        "status": if record.active { "active" } else { "idle" },
                    }),
                );
            }
            Ok(serde_json::json!({ "data": status }))
        });
    let manager = Arc::new(oc_plugin::PluginManager::with_host(Arc::new(host)));
    let input = serde_json::json!({
        "directory": state.location.directory.clone(),
        "worktree": state.location.directory.clone(),
        "project": { "id": state.location.project_id.clone() },
        "serverUrl": null,
    });
    for (spec, options) in declarations {
        let entry = match resolve_bootstrap_entry(&spec) {
            Ok(entry) => entry,
            Err(error) => {
                let report = oc_plugin::PluginLoadReport {
                    spec: spec.clone(),
                    summary: None,
                    error: Some(error.clone()),
                };
                state
                    .plugin_reports
                    .lock()
                    .expect("plugin report lock poisoned")
                    .push(report);
                tracing::warn!(plugin = %spec, ?error, "configured plugin could not be resolved");
                continue;
            }
        };
        let mut report = manager.load_local(entry, input.clone(), options);
        report.spec = spec.clone();
        state
            .plugin_reports
            .lock()
            .expect("plugin report lock poisoned")
            .push(report.clone());
        if let Some(error) = report.error {
            tracing::warn!(plugin = %spec, ?error, "configured plugin failed to load");
        } else if let Some(summary) = report.summary {
            tracing::info!(
                plugin = %spec,
                hooks = summary.hook_names.len(),
                tools = summary.tools.len(),
                "configured plugin loaded"
            );
        }
    }
    let reports = state
        .plugin_reports
        .lock()
        .expect("plugin report lock poisoned")
        .clone();
    state.provider_auth = Arc::new(crate::plugin_auth::from_plugin_reports_with_builtins(
        Arc::clone(&manager),
        &reports,
        internal_defaults,
    ));
    state.plugin_manager = Some(manager);
}

fn configured_plugin_declarations(
    config: &serde_json::Value,
) -> Vec<(String, Option<serde_json::Value>)> {
    let Some(plugins) = [config.get("plugin"), config.get("plugins")]
        .into_iter()
        .flatten()
        .find_map(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    plugins
        .iter()
        .filter_map(|plugin| match plugin {
            serde_json::Value::String(spec) => Some((spec.clone(), None)),
            serde_json::Value::Array(values) => Some((
                values.first()?.as_str()?.to_string(),
                values.get(1).cloned(),
            )),
            serde_json::Value::Object(value) => Some((
                value.get("package")?.as_str()?.to_string(),
                value.get("options").cloned(),
            )),
            _ => None,
        })
        .collect()
}

fn truthy_env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("True")
    )
}

fn resolve_bootstrap_entry(spec: &str) -> Result<String, String> {
    let target = if oc_plugin::loader::is_path_plugin_spec(spec) {
        spec.to_string()
    } else {
        oc_plugin::shared::resolve_plugin_target(spec)?
    };
    let entry =
        oc_plugin::shared::create_plugin_entry(spec, &target, oc_plugin::loader::KIND_SERVER)?;
    entry.entry.ok_or_else(|| {
        format!(
            "configured plugin {spec} does not expose a {} entrypoint",
            oc_plugin::loader::KIND_SERVER
        )
    })
}

/// Start the local config watcher alongside the production listener. Remote
/// config is resolved at bootstrap; local file edits are reloaded into the
/// served projection and published as `config.updated` events.
fn start_config_watcher(
    state: &crate::state::AppState,
    location: &Location,
    cancel: tokio_util::sync::CancellationToken,
) {
    let options = local_load_options(location);
    let mut watcher = match oc_config::ConfigReloadWatcher::with_default_debounce(options) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::debug!(?error, "config watcher unavailable for this listener");
            return;
        }
    };
    let state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => match watcher.poll() {
                    Ok(Some(instance)) => {
                        if let Ok(config) = serde_json::to_value(instance.config) {
                            state.stores.write().await.config = config.clone();
                            state.emit_event(crate::event::Event {
                                id: crate::event::event_id(),
                                metadata: None,
                                r#type: "config.updated".into(),
                                durable: None,
                                location: None,
                                data: serde_json::json!({ "config": config }),
                            });
                        }
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(?error, "config reload failed; retaining last good state"),
                }
            }
        }
    });
}

/// Resolve the active config before constructing the production state.
///
/// Config failures are non-fatal here, matching the server's existing
/// availability-first behavior: the listener still comes up and reports an
/// empty config projection instead of silently mixing a partially parsed one.
async fn load_config(location: &Location) -> serde_json::Value {
    let options = local_load_options(location);
    let remotes = remote_config_options();
    let loaded = if remotes.credentials.is_empty() {
        oc_config::load::load_instance_state(&options).map_err(|error| error.to_string())
    } else {
        oc_config::load::load_instance_state_with_remotes(&options, &remotes)
            .await
            .map_err(|error| error.to_string())
    };
    match loaded {
        Ok(instance) => {
            serde_json::to_value(instance.config).unwrap_or_else(|_| crate::state::default_config())
        }
        Err(error) => {
            tracing::warn!(?error, directory = %location.directory, "failed to resolve opencode config");
            crate::state::default_config()
        }
    }
}

fn local_load_options(location: &Location) -> oc_config::load::LoadOptions {
    let directory = Path::new(&location.directory);
    let worktree = oc_config::paths::find_up(&[".git"], directory, None)
        .into_iter()
        .next()
        .and_then(|git| git.parent().map(|path| path.to_string_lossy().into_owned()));
    oc_config::load::LoadOptions {
        directory: location.directory.clone(),
        worktree,
        ..Default::default()
    }
}

/// Convert the reference auth store's well-known entries into the config
/// loader's network boundary. OAuth/API provider credentials are deliberately
/// excluded: only enterprise well-known tokens authorize remote config.
fn remote_config_options() -> oc_config::load::RemoteConfigOptions {
    let store =
        oc_provider::auth::FileAuthStore::new(oc_project::util::global::Global::paths().data);
    let credentials = store
        .all()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(origin, info)| match info {
            Info::WellKnown(value) => Some(oc_config::load::RemoteConfigCredential::new(
                origin,
                value.key,
                value.token,
            )),
            _ => None,
        })
        .collect();
    oc_config::load::RemoteConfigOptions::new(credentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listen_binds_and_stops() {
        let previous_db = std::env::var_os("OPENCODE_DB");
        std::env::set_var("OPENCODE_DB", ":memory:");
        let opts = ListenOptions::new("127.0.0.1", 0);
        let listener = match listen(opts).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                if let Some(previous_db) = previous_db {
                    std::env::set_var("OPENCODE_DB", previous_db);
                } else {
                    std::env::remove_var("OPENCODE_DB");
                }
                return;
            }
            Err(error) => panic!("listen failed: {error:?}"),
        };
        let port = listener.port;
        assert!(port > 0);

        // The listener must accept TCP connections on the bound port.
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("tcp connect failed");
        drop(stream);

        listener.stop(false).await;
        if let Some(previous_db) = previous_db {
            std::env::set_var("OPENCODE_DB", previous_db);
        } else {
            std::env::remove_var("OPENCODE_DB");
        }
    }

    #[tokio::test]
    async fn listen_stops_when_injected_shutdown_triggers() {
        let previous_db = std::env::var_os("OPENCODE_DB");
        std::env::set_var("OPENCODE_DB", ":memory:");
        let shutdown = Signal::new();
        let opts = ListenOptions::new("127.0.0.1", 0).with_shutdown(shutdown.clone());
        let listener = match listen(opts).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                if let Some(previous_db) = previous_db {
                    std::env::set_var("OPENCODE_DB", previous_db);
                } else {
                    std::env::remove_var("OPENCODE_DB");
                }
                return;
            }
            Err(error) => panic!("listen failed: {error:?}"),
        };

        shutdown.trigger();
        tokio::time::timeout(std::time::Duration::from_secs(1), listener.stop(false))
            .await
            .expect("listener did not stop after shutdown signal");

        if let Some(previous_db) = previous_db {
            std::env::set_var("OPENCODE_DB", previous_db);
        } else {
            std::env::remove_var("OPENCODE_DB");
        }
    }

    #[test]
    fn production_bootstrap_loads_local_plugin_declarations() {
        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let spec = format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        bootstrap_plugins(&mut state, &serde_json::json!({ "plugin": [spec] }));
        assert!(state.plugin_manager.is_some());
        let reports = state.plugin_reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].summary.as_ref().unwrap().tools.len(), 1);
        assert!(reports[0].error.is_none());
    }

    #[test]
    fn production_bootstrap_installs_native_default_auth_hooks() {
        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        bootstrap_plugins(&mut state, &serde_json::json!({}));
        let methods = state.provider_auth.methods();
        assert!(methods.contains_key("openai"));
        assert!(methods.contains_key("xai"));
        assert!(methods.contains_key("github-copilot"));
        assert!(state.plugin_manager.is_none());
    }

    #[test]
    fn production_bootstrap_loads_v2_plugin_object_declarations() {
        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let spec = format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        bootstrap_plugins(
            &mut state,
            &serde_json::json!({
                "plugins": [{ "package": spec, "options": { "mode": "strict" } }]
            }),
        );
        let reports = state.plugin_reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].error.is_none(), "plugin failed: {reports:?}");
        assert_eq!(reports[0].summary.as_ref().unwrap().tools.len(), 1);
    }

    #[test]
    fn bootstrap_resolves_package_directory_entrypoint() {
        let directory = std::env::temp_dir().join(format!(
            "opencode-server-plugin-package-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("package.json"),
            r#"{"name":"server-package","main":"index.ts"}"#,
        )
        .unwrap();
        std::fs::write(directory.join("index.ts"), "export default () => ({})").unwrap();

        let entry = resolve_bootstrap_entry(&format!("file://{}", directory.display()))
            .expect("package directory should resolve");
        assert!(entry.ends_with("index.ts"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn production_bootstrap_projects_plugin_registrations() {
        let path = std::env::temp_dir().join(format!(
            "opencode-server-registration-{}.ts",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
import plugin from "opencode/plugin"
export default {
  id: "server-registration-test",
  server: async () => {
    plugin.command({ name: "review", template: "Review $ARGUMENTS" })
    plugin.skill({ name: "rust", description: "Rust helpers", content: "Use Rust" })
    return {}
  },
}
"#,
        )
        .expect("registration fixture");

        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let spec = format!("file://{}", path.display());
        bootstrap_plugins(&mut state, &serde_json::json!({ "plugin": [spec] }));

        let registrations = state.plugin_registrations.snapshot();
        assert_eq!(registrations.len(), 2);
        assert_eq!(
            registrations[0].plugin_id.as_deref(),
            Some("server-registration-test")
        );
        assert_eq!(registrations[0].kind, "command");
        assert_eq!(registrations[1].kind, "skill");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn production_bootstrap_wires_plugin_session_status_client() {
        let path = std::env::temp_dir().join(format!(
            "opencode-server-session-status-{}.ts",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
import plugin from "opencode/plugin"
export default {
  id: "session-status-test",
  server: async (input) => {
    const status = await input.client.session.status()
    plugin.command({
      name: status && typeof status === "object" ? "status-ok" : "status-bad",
      template: "status",
    })
    return {}
  },
}
"#,
        )
        .expect("session status fixture");

        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let spec = format!("file://{}", path.display());
        bootstrap_plugins(&mut state, &serde_json::json!({ "plugin": [spec] }));

        let reports = state.plugin_reports.lock().unwrap();
        assert!(reports[0].error.is_none(), "plugin failed: {reports:?}");
        let registrations = state.plugin_registrations.snapshot();
        assert_eq!(registrations[0].input["name"], "status-ok");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn production_bootstrap_wires_plugin_auth_into_provider_service() {
        let mut state = crate::state::AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        let spec = format!(
            "file://{}/tests/fixtures/auth.ts",
            env!("CARGO_MANIFEST_DIR").replace("oc-server", "oc-plugin")
        );
        bootstrap_plugins(&mut state, &serde_json::json!({ "plugin": [spec] }));

        let methods = state.provider_auth.methods();
        assert_eq!(methods["fixture-provider"][0].label, "Fixture OAuth");
        let authorization = state
            .provider_auth
            .authorize(
                "fixture-provider",
                &oc_provider::provider::auth::AuthorizeInput {
                    method: 0,
                    inputs: Some(std::collections::BTreeMap::from([(
                        "account".into(),
                        "ok".into(),
                    )])),
                },
            )
            .expect("plugin authorize should dispatch")
            .expect("OAuth method should return authorization");
        assert_eq!(
            authorization.method,
            oc_provider::provider::auth::CallbackMethod::Code
        );

        let mut auth = oc_provider::auth::MemoryAuthStore::new();
        state
            .provider_auth
            .callback(
                "fixture-provider",
                &oc_provider::provider::auth::CallbackInput {
                    method: 0,
                    code: Some("server-code".into()),
                },
                &mut auth,
            )
            .expect("plugin callback should persist credentials");
        use oc_provider::auth::AuthStore;
        assert!(auth.get("fixture-provider").unwrap().is_some());
    }
}

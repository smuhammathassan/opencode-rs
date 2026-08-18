//! Probe the exact TUI bootstrap calls against a live server URL (arg 1).
use oc_tui::client::{ClientConfig, HttpSdkClient, SdkClient};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::args().nth(1).expect("server url");
    let directory = std::env::args().nth(2);
    let client: Arc<dyn SdkClient> = Arc::new(
        HttpSdkClient::new(ClientConfig {
            url: url.clone(),
            directory,
            workspace: None,
        })
        .expect("client"),
    );

    match client.config_providers().await {
        Ok(cp) => println!(
            "config_providers OK: {} providers, default={:?}",
            cp.providers.len(),
            cp.default
        ),
        Err(e) => println!("config_providers ERR: {e:#}"),
    }
    match client.app_agents().await {
        Ok(a) => println!("app_agents OK: {} agents", a.len()),
        Err(e) => println!("app_agents ERR: {e:#}"),
    }
    match client.config_get().await {
        Ok(_c) => println!("config_get OK"),
        Err(e) => println!("config_get ERR: {e:#}"),
    }
    match client.session_status().await {
        Ok(s) => println!("session_status OK: {} entries", s.len()),
        Err(e) => println!("session_status ERR: {e:#}"),
    }
    let create = client
        .session_create(oc_tui::client::SessionCreateInput {
            directory: Some("/tmp/oc-proj".to_string()),
            workspace: None,
            agent: Some("build".to_string()),
            model: Some(oc_tui::types::ModelRef {
                id: "kimi-k3".into(),
                provider_id: "opencode-go".into(),
                variant: None,
            }),
            workspace_id: None,
        })
        .await;
    match create {
        Ok(session) => println!("session_create OK: id={:?}", session.id),
        Err(e) => println!("session_create ERR: {e:#}"),
    }
    match client.subscribe_events() {
        Ok(_) => println!("subscribe_events OK"),
        Err(e) => println!("subscribe_events ERR: {e:#}"),
    }
}

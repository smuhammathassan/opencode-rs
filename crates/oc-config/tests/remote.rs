mod common;

use common::TestHome;
use oc_config::load::{
    load_instance_state_with_remotes, LoadOptions, RemoteConfigCredential, RemoteConfigOptions,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn respond(listener: TcpListener, origin: String) {
    let endpoint = format!("{origin}/config");
    for _ in 0..2 {
        let (mut stream, _) = listener.accept().await.expect("accept remote request");
        let mut buffer = [0_u8; 8192];
        let size = stream.read(&mut buffer).await.expect("read remote request");
        let request = String::from_utf8_lossy(&buffer[..size]);
        let (status, body) = if request.starts_with("GET /.well-known/opencode") {
            (
                "200 OK",
                json!({
                    "config": {
                        "model": "remote/base",
                        "username": "remote-user"
                    },
                    "remote_config": {
                        "url": endpoint,
                        "headers": {
                            "Authorization": "Bearer {env:ORG_TOKEN}"
                        }
                    }
                })
                .to_string(),
            )
        } else if request.starts_with("GET /config")
            && request
                .to_ascii_lowercase()
                .contains("authorization: bearer secret-token")
        {
            (
                "200 OK",
                json!({
                    "config": {
                        "model": "remote/endpoint",
                        "instructions": ["remote-endpoint.md"]
                    }
                })
                .to_string(),
            )
        } else {
            ("401 Unauthorized", "{}".to_string())
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write remote response");
    }
}

#[tokio::test]
async fn loads_well_known_and_nested_remote_config_before_local_sources() {
    let home = TestHome::new();
    home.write_project(
        json!({
            "model": "local/model",
            "instructions": ["local.md"]
        }),
        "opencode.json",
    );

    let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping loopback remote fixture: {error}");
            return;
        }
        Err(error) => panic!("bind test remote config server: {error}"),
    };
    let origin = format!("http://{}", listener.local_addr().expect("server address"));
    let server = tokio::spawn(respond(listener, origin.clone()));

    let state = load_instance_state_with_remotes(
        &LoadOptions {
            directory: home.project.to_string_lossy().into_owned(),
            worktree: Some(home.home.to_string_lossy().into_owned()),
            username: Some("testuser".to_string()),
            ..Default::default()
        },
        &RemoteConfigOptions::new(vec![RemoteConfigCredential::new(
            &origin,
            "ORG_TOKEN",
            "secret-token",
        )]),
    )
    .await
    .expect("load remote config");

    server.await.expect("remote config server");
    assert_eq!(state.config.model.as_deref(), Some("local/model"));
    assert_eq!(state.config.username.as_deref(), Some("remote-user"));
    assert_eq!(
        state.config.instructions.as_deref(),
        Some(&["remote-endpoint.md".to_string(), "local.md".to_string()][..])
    );
}

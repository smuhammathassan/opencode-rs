//! Builtin remote workspace adapter.
//!
//! The remote target is intentionally transport-neutral: the workspace
//! runtime supplies the control-plane HTTP client after resolving this target.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::sync_api::{Method, ResponseKind, SyncApi, SyncHttpRequest, SyncHttpResponse};
use super::super::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};
use super::super::util::route;

const WORKSPACE_PATH: &str = "/experimental/workspace";

fn config(info: &WorkspaceInfo) -> anyhow::Result<(String, Vec<(String, String)>)> {
    let object = info
        .extra
        .as_ref()
        .and_then(|extra| extra.as_ref())
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow::anyhow!("Remote workspace requires an extra object"))?;
    let url = object
        .get("url")
        .or_else(|| object.get("baseUrl"))
        .or_else(|| object.get("baseURL"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Remote workspace requires extra.url"))?;
    let parsed =
        url::Url::parse(url).map_err(|error| anyhow::anyhow!("invalid remote URL: {error}"))?;
    if parsed.host_str().is_none() {
        anyhow::bail!("remote workspace URL must have a host");
    }
    // Remote targets must be HTTPS, except loopback hosts which are permitted
    // plain-HTTP for local development and in-process mock control planes.
    let is_loopback = parsed.host_str().is_some_and(|host| {
        host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1"
    });
    if parsed.scheme() != "https" && !is_loopback {
        anyhow::bail!("remote workspace URL must use HTTPS");
    }
    let headers = object
        .get("headers")
        .and_then(|value| value.as_object())
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| (name.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((url.trim_end_matches('/').to_string(), headers))
}

fn api(context: &WorkspaceAdapterContext) -> anyhow::Result<Arc<dyn SyncApi>> {
    context
        .sync_api
        .clone()
        .ok_or_else(|| anyhow::anyhow!("remote workspace lifecycle requires an injected SyncApi"))
}

fn remote_target(
    context: &WorkspaceAdapterContext,
) -> anyhow::Result<(String, Vec<(String, String)>)> {
    match context.target.as_ref() {
        Some(Target::Remote { url, headers }) => Ok((url.clone(), headers.clone())),
        Some(Target::Local { .. }) => {
            anyhow::bail!("remote workspace list requires a remote target")
        }
        None => anyhow::bail!("remote workspace list requires a configured target"),
    }
}

fn workspace_url(base: &str, id: Option<&str>) -> anyhow::Result<String> {
    let mut url = route(base, WORKSPACE_PATH)?;
    if let Some(id) = id {
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("remote workspace URL cannot contain path segments"))?
            .push(id);
    }
    Ok(url.to_string())
}

fn response_error(operation: &str, response: &SyncHttpResponse) -> anyhow::Error {
    let body = response.text.as_deref().unwrap_or_default().trim();
    if body.is_empty() {
        anyhow::anyhow!(
            "remote workspace {operation} failed with HTTP {}",
            response.status
        )
    } else {
        anyhow::anyhow!(
            "remote workspace {operation} failed with HTTP {}: {body}",
            response.status
        )
    }
}

fn create_body(info: &WorkspaceInfo) -> serde_json::Value {
    serde_json::json!({
        "id": info.id,
        "type": info.ty,
        "name": info.name,
        "branch": info.branch.as_ref().and_then(|branch| branch.clone()),
        "directory": info.directory.as_ref().and_then(|directory| directory.clone()),
        "extra": info.extra.as_ref().and_then(|extra| extra.clone()),
        "projectID": info.project_id,
    })
}

pub struct RemoteAdapter;

#[async_trait::async_trait]
impl WorkspaceAdapter for RemoteAdapter {
    fn name(&self) -> &'static str {
        "Remote"
    }
    fn description(&self) -> &'static str {
        "Connect to a remote OpenCode workspace"
    }

    async fn configure(
        &self,
        info: WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<WorkspaceInfo> {
        config(&info)?;
        Ok(info)
    }

    async fn create(
        &self,
        info: &WorkspaceInfo,
        _env: &BTreeMap<String, Option<String>>,
        _from: Option<&WorkspaceInfo>,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let (url, headers) = config(info)?;
        let response = api(context)?
            .execute(SyncHttpRequest {
                method: Method::Post,
                url: workspace_url(&url, None)?,
                headers,
                body: Some(create_body(info)),
                response: ResponseKind::Json,
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(response_error("creation", &response));
        }
        Ok(())
    }

    async fn list(
        &self,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
        let (url, headers) = remote_target(context)?;
        let response = api(context)?
            .execute(SyncHttpRequest {
                method: Method::Get,
                url: workspace_url(&url, None)?,
                headers,
                body: None,
                response: ResponseKind::Json,
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(response_error("listing", &response));
        }
        let values = response
            .json
            .and_then(|value| value.as_array().cloned())
            .ok_or_else(|| {
                anyhow::anyhow!("remote workspace listing returned a non-array JSON body")
            })?;
        values
            .into_iter()
            .map(serde_json::from_value::<WorkspaceListedInfo>)
            .filter_map(|item| match item {
                Ok(item)
                    if context
                        .project_id
                        .as_deref()
                        .is_none_or(|project_id| project_id == item.project_id) =>
                {
                    Some(Ok(item))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error.into())),
            })
            .collect()
    }

    async fn remove(
        &self,
        info: &WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let (url, headers) = config(info)?;
        let response = api(context)?
            .execute(SyncHttpRequest {
                method: Method::Delete,
                url: workspace_url(&url, Some(&info.id))?,
                headers,
                body: None,
                response: ResponseKind::Json,
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(response_error("removal", &response));
        }
        Ok(())
    }

    async fn target(
        &self,
        info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Target> {
        let (url, headers) = config(info)?;
        Ok(Target::Remote { url, headers })
    }
}

pub fn remote_adapter() -> std::sync::Arc<dyn WorkspaceAdapter> {
    std::sync::Arc::new(RemoteAdapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockApi {
        requests: Mutex<Vec<SyncHttpRequest>>,
    }

    #[async_trait::async_trait]
    impl SyncApi for MockApi {
        async fn execute(
            &self,
            request: SyncHttpRequest,
        ) -> Result<SyncHttpResponse, super::super::super::sync_api::SyncHttpError> {
            let method = request.method;
            self.requests.lock().unwrap().push(request);
            let response = match method {
                Method::Post => SyncHttpResponse {
                    status: 201,
                    text: None,
                    json: Some(serde_json::json!({ "id": "wrk_remote" })),
                },
                Method::Get => SyncHttpResponse {
                    status: 200,
                    text: None,
                    json: Some(serde_json::json!([
                        {
                            "type": "remote",
                            "name": "same-project",
                            "branch": null,
                            "directory": null,
                            "extra": { "url": "https://example.test" },
                            "projectID": "prj"
                        },
                        {
                            "type": "remote",
                            "name": "other-project",
                            "branch": null,
                            "directory": null,
                            "extra": { "url": "https://example.test" },
                            "projectID": "other"
                        }
                    ])),
                },
                Method::Delete => SyncHttpResponse {
                    status: 204,
                    text: None,
                    json: None,
                },
            };
            Ok(response)
        }

        async fn event_stream(
            &self,
            _url: &str,
            _headers: &[(String, String)],
        ) -> Result<
            Box<dyn tokio::io::AsyncBufRead + Send + Unpin>,
            super::super::super::sync_api::SyncHttpError,
        > {
            Err(super::super::super::sync_api::SyncHttpError::new(
                "not implemented in test",
                500,
                None,
            ))
        }
    }

    fn info(extra: serde_json::Value) -> WorkspaceInfo {
        WorkspaceInfo::from_row(
            "wrk_remote".into(),
            "remote".into(),
            "remote".into(),
            None,
            None,
            Some(extra),
            "prj".into(),
        )
    }

    #[tokio::test]
    async fn target_validates_https_and_headers() {
        let target = RemoteAdapter
            .target(&info(serde_json::json!({"url":"https://example.test/","headers":{"Authorization":"Bearer token"}})), &WorkspaceAdapterContext::default())
            .await
            .unwrap();
        assert_eq!(
            target,
            Target::Remote {
                url: "https://example.test".into(),
                headers: vec![("Authorization".into(), "Bearer token".into())]
            }
        );
    }

    #[tokio::test]
    async fn target_rejects_non_https_urls() {
        let error = RemoteAdapter
            .target(
                &info(serde_json::json!({"url":"http://example.test"})),
                &WorkspaceAdapterContext::default(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
    }

    #[tokio::test]
    async fn lifecycle_uses_injected_sync_api_contract() {
        let api = Arc::new(MockApi::default());
        let api_ref: Arc<dyn SyncApi> = api.clone();
        let info = info(serde_json::json!({
            "url": "https://example.test/instance/",
            "headers": { "Authorization": "Bearer token" }
        }));
        let target = Target::Remote {
            url: "https://example.test/instance".into(),
            headers: vec![("Authorization".into(), "Bearer token".into())],
        };
        let context = WorkspaceAdapterContext {
            project_id: Some("prj".into()),
            sync_api: Some(api_ref),
            target: Some(target),
            ..Default::default()
        };

        RemoteAdapter
            .create(&info, &BTreeMap::new(), None, &context)
            .await
            .unwrap();
        let listed = RemoteAdapter.list(&context).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "same-project");
        RemoteAdapter.remove(&info, &context).await.unwrap();

        let requests = api.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(
            requests[0].url,
            "https://example.test/instance/experimental/workspace"
        );
        assert_eq!(
            requests[0].headers,
            vec![("Authorization".into(), "Bearer token".into())]
        );
        assert_eq!(requests[0].body.as_ref().unwrap()["projectID"], "prj");
        assert_eq!(requests[1].method, Method::Get);
        assert_eq!(requests[2].method, Method::Delete);
        assert_eq!(
            requests[2].url,
            "https://example.test/instance/experimental/workspace/wrk_remote"
        );
    }

    #[tokio::test]
    async fn lifecycle_requires_transport_and_list_target() {
        let info = info(serde_json::json!({ "url": "https://example.test" }));
        let error = RemoteAdapter
            .create(
                &info,
                &BTreeMap::new(),
                None,
                &WorkspaceAdapterContext::default(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("injected SyncApi"));

        let api: Arc<dyn SyncApi> = Arc::new(MockApi::default());
        let error = RemoteAdapter
            .list(&WorkspaceAdapterContext {
                sync_api: Some(api),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("configured target"));
    }
}

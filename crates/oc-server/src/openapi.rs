//! Minimal OpenAPI document served at `/doc` and `/openapi.json`.
//!
//! The reference derives the spec with `OpenApi.fromApi(PublicApi)`
//! (reference/packages/opencode/src/server/routes/instance/httpapi/server.ts). We
//! generate an equivalent document from the route table: one `get/post/put/patch/
//! delete` entry per path with the route id as `operationId`.

use serde_json::{json, Map, Value};

use crate::route::{all_routes, Method};

/// Build the OpenAPI 3.0 document for the server surface.
pub fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for route in all_routes() {
        let key = openapi_path(route.path);
        let entry = paths.entry(key).or_insert_with(|| json!({}));
        let operation = json!({
            "operationId": route.id,
            "responses": { "200": { "description": "OK" } },
        });
        if let Some(object) = entry.as_object_mut() {
            object.insert(method_key(route.method).to_string(), operation);
        }
    }

    json!({
        "openapi": "3.0.0",
        "info": {
            "title": "opencode",
            "version": crate::version(),
            "description": "opencode api",
        },
        "paths": paths,
        "components": {
            "schemas": {},
        },
    })
}

fn method_key(method: Method) -> &'static str {
    match method {
        Method::Get => "get",
        Method::Post => "post",
        Method::Put => "put",
        Method::Patch => "patch",
        Method::Delete => "delete",
    }
}

/// Convert an Effect-style `:param` path into OpenAPI `{param}` syntax.
fn openapi_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            let mut param = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_alphanumeric() || next == '_' {
                    param.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push('{');
            out.push_str(&param);
            out.push('}');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_has_expected_shape() {
        let doc = document();
        assert_eq!(doc["info"]["title"], "opencode");
        assert!(doc["paths"].get("/api/session").is_some());
        assert!(doc["paths"].get("api/session").is_none());
    }

    #[test]
    fn paths_use_openapi_params() {
        assert_eq!(
            openapi_path("/api/session/:sessionID"),
            "/api/session/{sessionID}"
        );
    }
}

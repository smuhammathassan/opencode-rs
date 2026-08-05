//! Port of `reference/packages/core/src/tool/registry.ts`, `tools.ts` and
//! `application-tools.ts` — the V2 core tool registry.
//!
//! `CoreToolRegistry` owns Location-scoped local registrations layered over
//! the process-scoped `ApplicationTools` map; `materialize` derives
//! `ToolDefinition`s and a `settle` closure that executes the effective tool.

use std::collections::HashMap;

use crate::model::{ToolCall, ToolDefinition, ToolOutput, ToolResultValue};

use super::tool::{self, CoreContext, CoreTool};

/// A registration identity token (mirrors the reference `{}` identity objects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationToken(u64);

#[derive(Clone)]
struct Registration {
    identity: RegistrationToken,
    tool: CoreTool,
}

/// `ApplicationTools.Service` — process-scoped application registrations.
#[derive(Clone, Default)]
pub struct ApplicationTools {
    entries: HashMap<String, Registration>,
}

impl ApplicationTools {
    pub fn register(&mut self, tools: Vec<(String, CoreTool)>) -> Result<(), String> {
        for (name, _) in &tools {
            tool::validate_name(name)?;
        }
        let token = RegistrationToken(0);
        for (name, tool) in tools {
            self.entries.insert(
                name,
                Registration {
                    identity: token,
                    tool,
                },
            );
        }
        Ok(())
    }

    fn entries(&self) -> &HashMap<String, Registration> {
        &self.entries
    }
}

/// `Tools.Service` — the narrow registration-only capability.
#[derive(Clone, Default)]
pub struct ToolsService {
    registry: std::sync::Arc<std::sync::Mutex<CoreToolRegistry>>,
}

impl ToolsService {
    pub fn register(
        &self,
        tools: Vec<(String, CoreTool)>,
    ) -> Result<Box<dyn FnOnce() + Send>, String> {
        let mut guard = self.registry.lock().unwrap();
        guard.register(tools)
    }
}

/// `ToolRegistry.Service` from `reference/packages/core/src/tool/registry.ts:40`.
#[derive(Default)]
pub struct CoreToolRegistry {
    applications: ApplicationTools,
    local: HashMap<String, Vec<Registration>>,
    next_token: u64,
}

/// A scoped registration handle; dropping it removes the registration.
pub struct RegistrationGuard {
    registry: std::sync::Arc<std::sync::Mutex<CoreToolRegistry>>,
    names: Vec<String>,
    token: RegistrationToken,
}

impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        let mut guard = self.registry.lock().unwrap();
        for name in &self.names {
            if let Some(entries) = guard.local.get_mut(name) {
                entries.retain(|entry| entry.identity != self.token);
                if entries.is_empty() {
                    guard.local.remove(name);
                }
            }
        }
    }
}

impl CoreToolRegistry {
    pub fn new(applications: ApplicationTools) -> Self {
        CoreToolRegistry {
            applications,
            local: HashMap::new(),
            next_token: 1,
        }
    }

    pub fn with_applications() -> Self {
        CoreToolRegistry::new(ApplicationTools::default())
    }

    /// `ToolRegistry.register` from `reference/packages/core/src/tool/registry.ts:85`.
    pub fn register(
        &mut self,
        tools: Vec<(String, CoreTool)>,
    ) -> Result<Box<dyn FnOnce() + Send>, String> {
        if tools.is_empty() {
            return Ok(Box::new(|| {}));
        }
        for (name, _) in &tools {
            tool::validate_name(name)?;
        }
        let token = RegistrationToken(self.next_token);
        self.next_token += 1;
        let mut names = Vec::new();
        for (name, tool) in tools {
            self.local
                .entry(name.clone())
                .or_default()
                .push(Registration {
                    identity: token,
                    tool,
                });
            names.push(name);
        }
        Ok(Box::new(move || {
            let _ = names;
        }))
    }

    /// `ToolRegistry.materialize` from `reference/packages/core/src/tool/registry.ts:106`.
    pub fn materialize(&self, permissions: &[crate::util::Rule]) -> Materialization {
        let mut registrations = self.applications.entries().clone();
        for (name, entries) in &self.local {
            if let Some(registration) = entries.last() {
                registrations.insert(name.clone(), registration.clone());
            }
        }
        registrations.retain(|name, registration| {
            !wholly_disabled(&tool::permission(&registration.tool, name), permissions)
        });

        let definitions: Vec<ToolDefinition> = registrations
            .iter()
            .map(|(name, registration)| tool::definition(name, &registration.tool))
            .collect();

        let snapshot = registrations.clone();
        Materialization {
            definitions,
            settle: Box::new(move |input: &mut ExecuteInput, context: &mut CoreContext| {
                settle_registration(&snapshot, input, context)
            }),
        }
    }

    /// Register application-scoped tools (opencode.tools.register equivalent).
    pub fn register_application(&mut self, tools: Vec<(String, CoreTool)>) -> Result<(), String> {
        self.applications.register(tools)
    }
}

pub struct ExecuteInput {
    pub session_id: String,
    pub agent: String,
    pub assistant_message_id: String,
    pub call: ToolCall,
}

pub struct Materialization {
    pub definitions: Vec<ToolDefinition>,
    pub settle: Box<dyn Fn(&mut ExecuteInput, &mut CoreContext) -> Settlement>,
}

#[derive(Debug, Clone)]
pub enum Settlement {
    Error {
        value: String,
    },
    Ok {
        result: ToolResultValue,
        output: Option<ToolOutput>,
        output_paths: Vec<String>,
    },
}

fn settle_registration(
    registrations: &HashMap<String, Registration>,
    input: &mut ExecuteInput,
    context: &mut CoreContext,
) -> Settlement {
    let Some(registration) = registrations.get(&input.call.name) else {
        return Settlement::Error {
            value: format!("Unknown tool: {}", input.call.name),
        };
    };
    match tool::settle(&registration.tool, &input.call, context) {
        Ok(settled) => {
            let output = tool::project_output(settled);
            let bounded =
                match super::tool_output_store::bound(&super::tool_output_store::BoundInput {
                    session_id: input.session_id.clone(),
                    tool_call_id: input.call.id.clone(),
                    output,
                }) {
                    Ok(bounded) => bounded,
                    Err(message) => {
                        return Settlement::Error {
                            value: format!("Unable to bound tool output: {message}"),
                        }
                    }
                };
            let result = bounded.output.to_result_value();
            if matches!(result, ToolResultValue::Error { .. }) {
                if bounded.output_paths.is_empty() {
                    return Settlement::Error {
                        value: result_json(&result),
                    };
                }
                return Settlement::Ok {
                    result,
                    output: None,
                    output_paths: bounded.output_paths,
                };
            }
            Settlement::Ok {
                result,
                output: Some(bounded.output),
                output_paths: bounded.output_paths,
            }
        }
        Err(crate::model::ToolError::Failure(failure)) => Settlement::Error {
            value: failure.message,
        },
        Err(other) => Settlement::Error {
            value: other.message().to_string(),
        },
    }
}

fn result_json(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Error { value } => value.to_string(),
        ToolResultValue::Text { value } => value.to_string(),
        _ => "tool error".to_string(),
    }
}

fn wholly_disabled(action: &str, rules: &[crate::util::Rule]) -> bool {
    let rule = rules
        .iter()
        .rev()
        .find(|rule| crate::util::wildcard_match(action, &rule.permission));
    match rule {
        Some(rule) => rule.pattern == "*" && rule.action == "deny",
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{prop, Schema};
    use serde_json::Value as JsonValue;

    fn echo_tool() -> CoreTool {
        let input = Schema::struct_(vec![prop("text", Schema::plain_string())], "echo");
        let output = Schema::plain_string();
        tool::make(
            "Echoes text",
            input,
            output,
            None,
            None,
            None,
            |args, _ctx| Ok(args.get("text").cloned().unwrap_or(JsonValue::Null)),
        )
    }

    #[test]
    fn registers_and_materializes() {
        let mut registry = CoreToolRegistry::with_applications();
        registry
            .register(vec![("echo".to_string(), echo_tool())])
            .unwrap();
        let materialization = registry.materialize(&[]);
        let names: Vec<&str> = materialization
            .definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(names, vec!["echo"]);
    }

    #[test]
    fn settles_unknown_tool_as_error() {
        let registry = CoreToolRegistry::with_applications();
        let materialization = registry.materialize(&[]);
        let mut input = ExecuteInput {
            session_id: "ses".into(),
            agent: "build".into(),
            assistant_message_id: "msg".into(),
            call: ToolCall {
                id: "call_1".into(),
                name: "nope".into(),
                input: JsonValue::Object(Default::default()),
            },
        };
        let mut context = CoreContext {
            session_id: "ses".into(),
            agent: "build".into(),
            assistant_message_id: "msg".into(),
            tool_call_id: "call_1".into(),
            location_directory: "/tmp".into(),
            asks: vec![],
        };
        let settlement = (materialization.settle)(&mut input, &mut context);
        match settlement {
            Settlement::Error { value } => assert_eq!(value, "Unknown tool: nope"),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn settle_executes_tool_and_projects() {
        let mut registry = CoreToolRegistry::with_applications();
        registry
            .register(vec![("echo".to_string(), echo_tool())])
            .unwrap();
        let materialization = registry.materialize(&[]);
        let mut input = ExecuteInput {
            session_id: "ses".into(),
            agent: "build".into(),
            assistant_message_id: "msg".into(),
            call: ToolCall {
                id: "call_1".into(),
                name: "echo".into(),
                input: serde_json::json!({ "text": "hi" }),
            },
        };
        let mut context = CoreContext {
            session_id: "ses".into(),
            agent: "build".into(),
            assistant_message_id: "msg".into(),
            tool_call_id: "call_1".into(),
            location_directory: "/tmp".into(),
            asks: vec![],
        };
        let settlement = (materialization.settle)(&mut input, &mut context);
        match settlement {
            Settlement::Ok {
                result,
                output,
                output_paths,
            } => {
                assert!(output_paths.is_empty());
                assert!(output.is_some());
                assert_eq!(
                    result,
                    ToolResultValue::Text {
                        value: JsonValue::String("hi".into())
                    }
                );
            }
            other => panic!("expected ok, got {other:?}"),
        }
    }
}

//! Provider definition helper.
//! From reference/packages/llm/src/provider.ts

use crate::schema::Model;

/// `Provider.make(definition)` — a static provider definition.
/// From reference/packages/llm/src/provider.ts (`make`)
pub fn make<ModelFn>(id: String, model: ModelFn) -> ProviderDefinition<ModelFn>
where
    ModelFn: Fn(String) -> Model,
{
    ProviderDefinition { id, model }
}

/// `Definition` — a provider id plus a model factory.
/// From reference/packages/llm/src/provider.ts (`Definition`)
pub struct ProviderDefinition<ModelFn> {
    pub id: String,
    pub model: ModelFn,
}

/// `ProviderOptions` for the provider definition surface.
/// From reference/packages/llm/src/provider.ts (`ModelOptions`)
pub type ProviderModelOptions = crate::schema::ModelDefaults;

/// `Provider.model` shorthand.
pub fn model(id: impl Into<String>, model: Model) -> Model {
    let _ = id;
    model
}

/// From reference/packages/opencode/src/util/local-context.ts
///
/// Async-local context, mirroring `AsyncLocalStorage`'s `use`/`provide`. The
/// reference's storage propagates to tasks spawned inside `provide`; tokio's
/// `task_local!` (thread-backed) does not, so cross-task propagation is left
/// to the caller or a future context-propagation crate.
/// `TODO(integration): true async-context propagation across spawned tasks`.
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use tokio::task_local;

type Boxed = Arc<dyn Any + Send + Sync>;

#[derive(Default)]
struct ContextMap(HashMap<TypeId, Boxed>);

task_local! {
    static CONTEXT: ContextMap;
}

#[derive(Debug)]
pub struct NotFound {
    pub name: &'static str,
}

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "No context found for {}", self.name)
    }
}

impl std::error::Error for NotFound {}

#[derive(Clone, Copy)]
pub struct LocalContext {
    name: &'static str,
}

impl LocalContext {
    pub const fn new(name: &'static str) -> Self {
        LocalContext { name }
    }

    pub fn use_val<T: Clone + Send + Sync + 'static>(&self) -> Result<T, NotFound> {
        let result = CONTEXT.try_with(|context| {
            context
                .0
                .get(&TypeId::of::<T>())
                .and_then(|value| value.downcast_ref::<T>().cloned())
        });
        match result {
            Ok(Some(value)) => Ok(value),
            _ => Err(NotFound { name: self.name }),
        }
    }

    pub async fn provide<T, F, R>(&self, value: T, fut: F) -> R
    where
        T: Send + Sync + 'static,
        F: Future<Output = R>,
    {
        let mut child = CONTEXT
            .try_with(|context| ContextMap(context.0.clone()))
            .unwrap_or_default();
        child.0.insert(TypeId::of::<T>(), Arc::new(value));
        CONTEXT.scope(child, fut).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn use_raises_not_found_outside_provide() {
        let ctx = LocalContext::new("TestContext");
        let err = ctx.use_val::<u32>().unwrap_err();
        assert_eq!(err.to_string(), "No context found for TestContext");
    }

    #[tokio::test]
    async fn provide_makes_value_available() {
        let ctx = LocalContext::new("TestContext");
        let result = ctx
            .provide(42u32, async { ctx.use_val::<u32>().unwrap() })
            .await;
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn nested_provide_overrides_then_restores() {
        let ctx = LocalContext::new("TestContext");
        let result = ctx
            .provide(1u32, async {
                let inner = ctx
                    .provide(2u32, async { ctx.use_val::<u32>().unwrap() })
                    .await;
                (ctx.use_val::<u32>().unwrap(), inner)
            })
            .await;
        assert_eq!(result, (1, 2));
    }

    #[tokio::test]
    async fn provide_works_inside_spawned_tasks() {
        let ctx = LocalContext::new("TestContext");
        let result = ctx
            .provide(7u32, async move {
                let ctx = ctx;
                tokio::spawn(async move {
                    ctx.provide(9u32, async move { ctx.use_val::<u32>().unwrap() })
                        .await
                })
                .await
                .unwrap()
            })
            .await;
        assert_eq!(result, 9);
    }
}

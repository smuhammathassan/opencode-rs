/// From reference/packages/opencode/src/project/instance-runtime.ts
///
/// Bridge for callers that cannot hold a service reference; thin wrappers over
/// `InstanceStore`. In the reference these go through the Effect runtime.
use crate::project::instance_context::InstanceContext;
use crate::project::instance_store::{InstanceStore, LoadInput};

pub async fn load(store: &InstanceStore, input: LoadInput) -> Result<InstanceContext, String> {
    store.load(input).await
}

pub async fn dispose_instance(store: &InstanceStore, ctx: &InstanceContext) {
    store.dispose(ctx).await;
}

pub async fn dispose_all_instances(store: &InstanceStore) {
    store.dispose_all().await;
}

pub async fn reload_instance(
    store: &InstanceStore,
    input: LoadInput,
) -> Result<InstanceContext, String> {
    store.reload(input).await
}

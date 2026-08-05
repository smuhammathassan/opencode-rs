/// From reference/packages/opencode/src/project/bootstrap-service.ts
use tokio::sync::broadcast;

use crate::project::instance_context::InstanceContext;

pub trait Interface {
    fn run(&self, ctx: &InstanceContext, shutdown: broadcast::Sender<()>) -> Vec<tokio::task::JoinHandle<()>>;
}

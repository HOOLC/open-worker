use std::{future::Future, pin::Pin};

use super::super::{ToolDefinition, ToolError, ToolExecutionResult, ToolInvocation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolConcurrency {
    Parallel,
    Exclusive,
    Resource {
        key: String,
        access: ToolResourceAccess,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolResourceAccess {
    Read,
    Write,
}

pub trait ToolPort: Send + Sync {
    fn definitions(&self, selected: &[String]) -> Result<Vec<ToolDefinition>, ToolError>;

    fn concurrency(&self, _invocation: &ToolInvocation) -> ToolConcurrency {
        ToolConcurrency::Parallel
    }

    fn execute<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<ToolExecutionResult, ToolError>> + Send + 'a>>;
}

pub use ToolPort as ToolExecutor;

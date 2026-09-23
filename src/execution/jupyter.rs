//! Linux kernel selection and supervised execution of prepared pages.

mod deadline;
mod discovery;
mod execution;
mod output;
mod page;
mod process;
mod session;
mod transport;

#[cfg(test)]
mod tests;

use crate::ir::SourceLocation;

use super::{ExecutionFailure, ExecutionFailureKind};

#[derive(Clone)]
struct FailureSource {
    collection: String,
    source: SourceLocation,
}

impl FailureSource {
    fn for_cell(&self, cell: &crate::execution::PreparedCell) -> Self {
        let mut source = self.clone();
        source.source.span = Some(cell.cell.span);
        source
    }

    fn failure(&self, kind: ExecutionFailureKind, message: &str) -> ExecutionFailure {
        let mut diagnostic = kind.to_diagnostic(&self.collection, self.source.clone());
        diagnostic.message = message.into();
        ExecutionFailure {
            kind,
            diagnostics: vec![diagnostic],
            cleanup_diagnostics: Vec::new(),
        }
    }
}

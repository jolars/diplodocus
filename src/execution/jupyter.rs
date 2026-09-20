//! Linux kernel selection and supervised sessions, below the page executor.

mod discovery;
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

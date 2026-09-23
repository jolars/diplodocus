//! Preserve warning ownership across transport, consumer, and cleanup failures.

use crate::diagnostics::Diagnostic;
use crate::execution::ExecutionFailure;
use crate::execution::jupyter::output::OutputReducer;
use crate::execution::output_safety::ExecutionDiagnostic;

#[derive(Debug)]
pub(in crate::execution::jupyter) struct FailureDetails {
    pub failure: ExecutionFailure,
    pub before: Vec<Diagnostic>,
    pub previous: Vec<ExecutionDiagnostic>,
    pub pending: Vec<ExecutionDiagnostic>,
    pub cell: Option<usize>,
    pub consumer: bool,
    pub collection: String,
}

pub(in crate::execution::jupyter) type SessionFailure = Box<FailureDetails>;

impl From<ExecutionFailure> for SessionFailure {
    fn from(failure: ExecutionFailure) -> Self {
        Box::new(FailureDetails {
            failure,
            before: vec![],
            previous: vec![],
            pending: vec![],
            cell: None,
            consumer: false,
            collection: String::new(),
        })
    }
}

impl FailureDetails {
    pub fn into_failure(self) -> ExecutionFailure {
        let Self {
            mut failure,
            before,
            previous,
            pending,
            consumer,
            collection,
            ..
        } = self;
        if !consumer {
            failure.diagnostics.splice(
                0..0,
                previous
                    .iter()
                    .chain(&pending)
                    .map(|d| d.to_diagnostic(&collection)),
            );
        }
        failure.diagnostics.splice(0..0, before);
        failure
    }

    pub fn with_reduction(self, reducer: &OutputReducer) -> ExecutionFailure {
        let Self {
            mut failure,
            before,
            pending,
            cell,
            consumer,
            collection,
            ..
        } = self;
        if !consumer {
            let mut warnings = reducer.diagnostics();
            // A liveness failure can race an already-completed synchronous callback.
            // Its warnings then belong to the reducer, not to this pending tail.
            if cell.is_some_and(|cell| cell >= reducer.cell_count()) {
                warnings.extend(pending.iter().map(|d| d.to_diagnostic(&collection)));
            }
            failure.diagnostics.splice(0..0, warnings);
        }
        failure.diagnostics.splice(0..0, before);
        failure
    }
}

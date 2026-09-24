//! Private, whole-page execution artifacts. Cache data never grants rendering
//! trust: candidates pass the active validators before the engine accepts them.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::identity::{self, CanonicalError, CanonicalValue, ExecutionIdentity};
use super::*;
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};

mod codec;
mod storage;
#[cfg(test)]
mod tests;

pub(crate) struct Candidate {
    pub page: ValidatedPage,
    pub assets: BTreeMap<String, Vec<u8>>,
}

pub(crate) enum Lookup {
    Miss,
    Rejected,
    Hit(Box<Candidate>),
}

/// The worker owns only reads and in-memory validation. Awaiting its completion
/// during cancellation cannot race an asset rollback or a fresh submission.
pub(crate) async fn lookup(
    root: PathBuf,
    identity: ExecutionIdentity,
    prepared: PreparedExecution,
) -> Lookup {
    tokio::task::spawn_blocking(move || storage::lookup(&root, identity.key_input(), &prepared))
        .await
        .unwrap_or(Lookup::Rejected)
}

pub(crate) async fn publish(
    root: PathBuf,
    identity: ExecutionIdentity,
    prepared: PreparedExecution,
    result: &PageExecutionResult,
) -> Vec<Diagnostic> {
    let page = result.validated();
    let source = page.record().page.clone();
    let encoded = match codec::encode(&prepared, identity.key_input(), page) {
        Ok(bytes) => bytes,
        Err(_) => return vec![warning(&source, DiagnosticCode::ExecutionCacheUnavailable)],
    };
    let assets = result
        .staged_assets()
        .iter()
        .map(|asset| {
            (
                asset.reference.fingerprint.value.clone(),
                asset.path.clone(),
            )
        })
        .collect();
    let code = tokio::task::spawn_blocking(move || {
        storage::publish(&root, identity.key_input(), &prepared, &encoded, assets)
    })
    .await
    .unwrap_or(Some(DiagnosticCode::ExecutionCacheUnavailable));
    code.into_iter()
        .map(|code| warning(&source, code))
        .collect()
}

pub(crate) fn warning(page: &ExecutionPage, code: DiagnosticCode) -> Diagnostic {
    let message = match code {
        DiagnosticCode::InvalidExecutionCache => {
            "The complete cached page could not be validated; executing it again."
        }
        DiagnosticCode::NonDeterministicExecution => {
            "The same execution inputs produced a different result; the existing valid cache entry was kept."
        }
        _ => "The execution cache could not be published; the successful page remains usable.",
    };
    Diagnostic::new(code, Severity::Warning, message)
        .with_entity(DiagnosticEntity::Content {
            id: page.collection.clone(),
        })
        .with_source(DiagnosticSource::Repository {
            repository: page.source.repository.clone(),
            path: page.source.path.clone(),
        })
}

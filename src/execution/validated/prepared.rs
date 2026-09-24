use super::*;

/// Bound original source, prepared request, and authored output context.
///
/// This token proves source/request agreement, not command authorization. The
/// original source is prepared again to check the request and complete authored
/// anchor set. Generated output cannot introduce additional targets.
///
/// ```compile_fail
/// use diplodocus::execution::{PreparedExecution, PageExecutionRequest};
/// use diplodocus::execution::output_safety::AuthoredOutputContext;
/// fn construct(request: PageExecutionRequest, context: AuthoredOutputContext) -> PreparedExecution {
///     PreparedExecution { request, source: vec![], context }
/// }
/// ```
/// ```compile_fail
/// use diplodocus::execution::PreparedExecution;
/// fn mutate(prepared: &mut PreparedExecution) { prepared.request().cells.clear(); }
/// ```
/// ```compile_fail
/// use diplodocus::execution::PreparedExecution;
/// fn serialize<T: serde::Serialize>() {}
/// serialize::<PreparedExecution>();
/// ```
/// ```compile_fail
/// use diplodocus::execution::PreparedExecution;
/// let _: PreparedExecution = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExecution {
    request: PageExecutionRequest,
    source: Vec<u8>,
    context: AuthoredOutputContext,
}
impl PreparedExecution {
    /// Borrow the request that was checked against the original bytes.
    pub fn request(&self) -> &PageExecutionRequest {
        &self.request
    }
    /// Original authored bytes, including prose and option declarations.
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    /// Bound authored identity and the preparer's captured anchors.
    pub fn context(&self) -> &AuthoredOutputContext {
        &self.context
    }

    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "The production engine is supported only on Linux."
        )
    )]
    pub(crate) fn checked(
        request: PageExecutionRequest,
        source: Vec<u8>,
        context: AuthoredOutputContext,
    ) -> Result<Self, RecordValidationError> {
        if request.page.source_fingerprint != fingerprint_bytes(&source)
            || context.source() != &request.page.source
            || context.collection() != request.page.collection
        {
            return Err(RecordValidationError::Association);
        }
        let preparation = identity::validate_prepared(&request, &source)
            .map_err(|_| RecordValidationError::Association)?;
        if context.anchors() != &preparation.authored_anchors {
            return Err(RecordValidationError::Association);
        }
        let parent = std::path::Path::new(request.page.source.path.as_str())
            .parent()
            .and_then(|p| p.to_str())
            .filter(|p| !p.is_empty());
        if request.page.working_directory.as_ref().map(|p| p.as_str()) != parent {
            return Err(RecordValidationError::Association);
        }
        Ok(Self {
            request,
            source,
            context,
        })
    }
}

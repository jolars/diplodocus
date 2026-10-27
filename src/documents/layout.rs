use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::MetadataValue;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageLayout {
    Docs,
    Full,
}

impl PageLayout {
    pub(crate) fn from_metadata(metadata: Option<&MetadataValue>) -> Result<Self, Box<Diagnostic>> {
        let Some(MetadataValue::Mapping { entries, .. }) = metadata else {
            return Ok(Self::Docs);
        };
        let mut declarations = entries.iter().filter(|entry| entry.key.value == "layout");
        let Some(entry) = declarations.next() else {
            return Ok(Self::Docs);
        };
        let duplicates: Vec<_> = declarations.map(|entry| entry.span).collect();
        let layout = match &entry.value {
            MetadataValue::Scalar { value, .. } if value == "docs" => Some(Self::Docs),
            MetadataValue::Scalar { value, .. } if value == "full" => Some(Self::Full),
            _ => None,
        };
        if duplicates.is_empty()
            && let Some(layout) = layout
        {
            return Ok(layout);
        }
        Err(Box::new(Diagnostic {
            code: DiagnosticCode::InvalidPageLayout,
            severity: Severity::Error,
            message: if duplicates.is_empty() {
                "Page layout must be `docs` or `full`."
            } else {
                "Page layout must be declared only once."
            }
            .into(),
            span: Some(entry.span),
            related_spans: duplicates,
            related_entity: None,
            source: None,
        }))
    }
}

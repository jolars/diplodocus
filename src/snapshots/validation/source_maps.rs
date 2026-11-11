use super::*;

pub(super) fn validate(
    record: &ResolvedDocument,
    document: &SourcedDocument,
    authored: bool,
) -> Result<(), SnapshotError> {
    if record.source_ranges.is_empty() {
        return Ok(());
    }
    require(
        authored
            && matches!(
                document.source_format,
                DocumentFormat::Authored {
                    format: crate::documents::AuthoredFormat::Gfm
                        | crate::documents::AuthoredFormat::Qmd
                }
            ),
        "source map document",
    )?;
    let text = document
        .raw_source
        .as_ref()
        .ok_or(SnapshotError::Invalid("source map source"))?;
    let source = document
        .source_location
        .as_ref()
        .ok_or(SnapshotError::Invalid("source map location"))?;
    let mut cursor = 0;
    for range in &record.source_ranges {
        let bytes = text
            .get(range.expanded.start..range.expanded.end)
            .ok_or(SnapshotError::Invalid("source map expanded range"))?;
        let original = range
            .original
            .span
            .ok_or(SnapshotError::Invalid("source map original range"))?;
        require(
            range.expanded.start == cursor
                && original.end.checked_sub(original.start) == Some(bytes.len())
                && range.line > 0
                && range.line - 1 <= original.start,
            "source map range association",
        )?;
        let mut paths = BTreeSet::new();
        require(range.include_chain.len() < 128, "source map include depth")?;
        for directive in &range.include_chain {
            require(
                directive.repository == source.repository
                    && directive.span.is_some_and(|span| span.start < span.end)
                    && paths.insert(&directive.path),
                "source map include chain",
            )?;
        }
        require(
            range.original.repository == source.repository
                && !paths.contains(&range.original.path)
                && range.include_chain.first().unwrap_or(&range.original).path == source.path,
            "source map origin",
        )?;
        cursor = range.expanded.end;
    }
    require(cursor == text.len(), "source map coverage")
}

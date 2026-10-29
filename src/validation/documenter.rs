//! Documenter cross-references resolve against retained pages and canonical API items.

use crate::diagnostics::DiagnosticCode;
use crate::documents::{AuthoredFormat, inline_text};
use crate::ir::*;

use super::{ReferenceTarget, resolve_item};

pub(crate) fn reference(
    workspace: &Workspace,
    document: &SourcedDocument,
    owner: Option<&str>,
    spelling: &str,
) -> Option<Result<ReferenceTarget, DiagnosticCode>> {
    if !matches!(
        document.source_format,
        DocumentFormat::Authored {
            format: AuthoredFormat::Documenter
        }
    ) && !matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "julia-markdown")
    {
        return None;
    }
    let target = spelling.strip_prefix("@ref ")?.trim();
    let quoted = target.strip_prefix('"').and_then(|s| s.strip_suffix('"'));
    let code = target.strip_prefix('`').and_then(|s| s.strip_suffix('`'));
    let title = quoted.or(code).unwrap_or(target);
    let collection = workspace.pages.values().find_map(|page| {
        if std::ptr::eq(&page.document, document)
            && let PageKind::Authored { collection } = &page.kind
        {
            return Some(collection);
        }
        None
    });
    let headings = || {
        let mut found = Vec::new();
        for (id, page) in &workspace.pages {
            if !matches!(
                page.document.source_format,
                DocumentFormat::Authored {
                    format: AuthoredFormat::Documenter
                }
            ) {
                continue;
            }
            if let Some(collection) = collection {
                if !matches!(&page.kind, PageKind::Authored { collection: other } if other == collection)
                {
                    continue;
                }
            } else if owner.is_some_and(|owner| {
                page.owner
                    != (ContentOwner::Package {
                        package: owner.into(),
                    })
            }) {
                continue;
            }
            collect(
                &page.document.document.blocks,
                title,
                quoted.is_some(),
                id,
                &mut found,
            );
        }
        match found.as_slice() {
            [] => Ok(None),
            [target] => Ok(Some(target.clone())),
            _ => Err(DiagnosticCode::UnresolvedDocumentReference),
        }
    };
    Some(if quoted.is_some() {
        headings().and_then(|target| target.ok_or(DiagnosticCode::UnresolvedDocumentReference))
    } else if code.is_some() {
        match resolve_item(workspace, owner, title) {
            Ok(item) => Ok(ReferenceTarget::Item { item }),
            Err(DiagnosticCode::UnresolvedItemReference) => headings()
                .and_then(|target| target.ok_or(DiagnosticCode::UnresolvedDocumentReference)),
            Err(code) => Err(code),
        }
    } else {
        // Legacy unquoted targets select an existing heading label before an API name.
        match headings() {
            Ok(Some(target)) => Ok(target),
            Ok(None) => {
                resolve_item(workspace, owner, target).map(|item| ReferenceTarget::Item { item })
            }
            Err(code) => Err(code),
        }
    })
}

fn collect(
    blocks: &[Block],
    title: &str,
    quoted: bool,
    page: &str,
    found: &mut Vec<ReferenceTarget>,
) {
    for block in blocks {
        match block {
            Block::Heading {
                attributes,
                inlines,
                ..
            } => {
                if let Some(id) = &attributes.identifier
                    && (inline_text(inlines)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        == title
                        || !quoted && id.value == title)
                {
                    found.push(ReferenceTarget::Page {
                        page: page.into(),
                        fragment: Some(id.value.clone()),
                    });
                }
            }
            Block::Container { blocks, .. }
            | Block::Callout { blocks, .. }
            | Block::BlockQuote { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. } => collect(blocks, title, quoted, page, found),
            Block::List { items, .. } => {
                for item in items {
                    collect(&item.blocks, title, quoted, page, found);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect(&cell.blocks, title, quoted, page, found);
                    }
                }
            }
            _ => {}
        }
    }
}

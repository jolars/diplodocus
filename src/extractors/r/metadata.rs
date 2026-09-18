use std::collections::BTreeMap;

use arity_parser::{ast::AstNode, dcf};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::SourceLocation;

use super::{diagnostic, located};

/// Static DESCRIPTION metadata with the original fields and their evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMetadata {
    /// Published R package name, independent of its workspace ID.
    pub name: String,
    /// Declared R package version.
    pub version: String,
    /// Short package title.
    pub title: String,
    /// Package description when supplied.
    pub description: Option<String>,
    /// Unevaluated license declaration.
    pub license: Option<String>,
    /// Dependencies grouped by their DESCRIPTION field, including R in Depends.
    pub dependencies: BTreeMap<String, Vec<RDependency>>,
    /// All original fields, including unevaluated Authors@R.
    pub fields: BTreeMap<String, RMetadataField>,
    /// Original metadata file.
    pub source: SourceLocation,
}

/// One DCF field, retaining folded text and its exact enclosing source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RMetadataField {
    /// Continuation lines folded by the DCF parser.
    pub value: String,
    /// Exact source field.
    pub source: SourceLocation,
}

/// One static dependency declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RDependency {
    /// Package name, or R for the interpreter requirement.
    pub name: String,
    /// Constraints in declared order.
    pub constraints: Vec<RVersionConstraint>,
    /// Exact dependency source range.
    pub source: SourceLocation,
}

/// An unevaluated R version constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RVersionConstraint {
    /// One of <, <=, ==, >=, or >.
    pub operator: String,
    /// Version spelling in R syntax.
    pub version: String,
    /// Exact constraint source range.
    pub source: SourceLocation,
}

pub(super) fn parse(
    text: &str,
    source: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<RMetadata> {
    let before = diagnostics.len();
    let parsed = dcf::parse(text);
    for error in &parsed.diagnostics {
        diagnostics.push(diagnostic(
            DiagnosticCode::RMetadata,
            Severity::Error,
            &error.message,
            &located(source, error.start, error.end),
        ));
    }
    let document = parsed.document();
    if document.records().count() != 1 {
        diagnostics.push(diagnostic(
            DiagnosticCode::RMetadata,
            Severity::Error,
            "DESCRIPTION must contain exactly one DCF record.",
            source,
        ));
    }
    let mut fields = BTreeMap::new();
    let mut dependencies = BTreeMap::new();
    for field in document.fields() {
        let name = field.name().to_string();
        let range = field.syntax().text_range();
        let location = located(source, usize::from(range.start()), usize::from(range.end()));
        let value = field.folded_value();
        if fields
            .insert(
                name.clone(),
                RMetadataField {
                    value: value.clone(),
                    source: location.clone(),
                },
            )
            .is_some()
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::RMetadata,
                Severity::Error,
                format!("Duplicate DESCRIPTION field `{name}`."),
                &location,
            ));
        }
        if dcf::is_dependency_field(&name) {
            let mut entries = vec![];
            for entry in dcf::dependency_entries(&field) {
                let entry_source = located(
                    source,
                    usize::from(entry.range.start()),
                    usize::from(entry.range.end()),
                );
                let raw = &text[entry_source.span.unwrap().start..entry_source.span.unwrap().end];
                let compact: String = raw
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('#'))
                    .flat_map(str::chars)
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let mut constraints = vec![];
                for constraint in &entry.constraints {
                    let operator = match constraint.op {
                        dcf::VersionOp::Lt => "<",
                        dcf::VersionOp::Le => "<=",
                        dcf::VersionOp::Eq => "==",
                        dcf::VersionOp::Ge => ">=",
                        dcf::VersionOp::Gt => ">",
                        dcf::VersionOp::Ne => "!=",
                    };
                    constraints.push(RVersionConstraint {
                        operator: operator.into(),
                        version: constraint.version.to_string(),
                        source: located(
                            source,
                            usize::from(constraint.range.start()),
                            usize::from(constraint.range.end()),
                        ),
                    });
                }
                let mut expected = entry.name.to_string();
                if entry.constraint_text.is_some() {
                    expected.push('(');
                    expected.push_str(
                        &constraints
                            .iter()
                            .map(|c| format!("{}{}", c.operator, c.version))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    expected.push(')');
                }
                if !package_name(&entry.name)
                    || entry.malformed_constraint()
                    || compact != expected
                    || constraints
                        .iter()
                        .any(|c| !version(&c.version) || c.operator == "!=")
                {
                    diagnostics.push(diagnostic(
                        DiagnosticCode::RMetadata,
                        Severity::Error,
                        format!("Malformed dependency declaration for `{}`.", entry.name),
                        &entry_source,
                    ));
                }
                entries.push(RDependency {
                    name: entry.name.to_string(),
                    constraints,
                    source: entry_source,
                });
            }
            dependencies.insert(name, entries);
        }
    }
    for name in ["Package", "Version", "Title"] {
        if fields
            .get(name)
            .is_none_or(|field| field.value.trim().is_empty())
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::RMetadata,
                Severity::Error,
                format!("DESCRIPTION requires a nonempty `{name}` field."),
                source,
            ));
        }
    }
    for (name, valid) in [
        ("Package", package_name as fn(&str) -> bool),
        ("Version", version),
    ] {
        if let Some(field) = fields.get(name)
            && !valid(&field.value)
        {
            diagnostics.push(diagnostic(
                DiagnosticCode::RMetadata,
                Severity::Error,
                format!("Invalid DESCRIPTION `{name}`."),
                &field.source,
            ));
        }
    }
    if let Some(field) = fields.get("Encoding")
        && !field.value.eq_ignore_ascii_case("UTF-8")
    {
        diagnostics.push(diagnostic(
            DiagnosticCode::RMetadata,
            Severity::Error,
            "Only UTF-8 package encoding is supported.",
            &field.source,
        ));
    }
    if diagnostics.len() != before {
        return None;
    }
    Some(RMetadata {
        name: fields["Package"].value.clone(),
        version: fields["Version"].value.clone(),
        title: fields["Title"].value.clone(),
        description: fields.get("Description").map(|f| f.value.clone()),
        license: fields.get("License").map(|f| f.value.clone()),
        dependencies,
        fields,
        source: source.clone(),
    })
}

fn package_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && !name.ends_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

fn version(value: &str) -> bool {
    let parts: Vec<_> = value.split(['.', '-']).collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

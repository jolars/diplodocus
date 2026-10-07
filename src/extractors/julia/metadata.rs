use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{SourceLocation, SourceSpan};

use super::diagnostic;

/// Static Project.toml values; no manifest or Julia environment is consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaMetadata {
    /// Published package name.
    pub name: String,
    /// Validated package UUID.
    pub uuid: String,
    /// Declared SemVer version when supplied.
    pub version: Option<String>,
    /// Maintained author names.
    pub authors: Vec<String>,
    /// Direct dependency UUIDs.
    pub dependencies: BTreeMap<String, String>,
    /// Julia/Pkg compatibility syntax retained without Cargo interpretation.
    pub compatibility: BTreeMap<String, String>,
    /// Every original top-level field with source evidence.
    pub fields: BTreeMap<String, JuliaMetadataField>,
    /// Portable metadata file location.
    pub source: SourceLocation,
}

/// A top-level metadata value and its exact TOML source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JuliaMetadataField {
    /// Original value represented as portable JSON.
    pub value: serde_json::Value,
    /// Exact value range in the metadata file.
    pub source: SourceLocation,
}

pub(super) fn parse(
    text: &str,
    source: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<JuliaMetadata> {
    let values: BTreeMap<String, toml::Spanned<toml::Value>> = match toml::from_str(text) {
        Ok(values) => values,
        Err(error) => {
            let mut location = source.clone();
            location.span = error.span().map(|r| SourceSpan {
                start: r.start,
                end: r.end,
            });
            diagnostics.push(diagnostic(
                DiagnosticCode::JuliaMetadata,
                Severity::Error,
                "Project metadata is not valid TOML.",
                &location,
            ));
            return None;
        }
    };
    let fields: BTreeMap<_, _> = values
        .into_iter()
        .map(|(name, value)| {
            let range = value.span();
            (
                name,
                JuliaMetadataField {
                    value: serde_json::to_value(value.into_inner()).unwrap(),
                    source: SourceLocation {
                        span: Some(SourceSpan {
                            start: range.start,
                            end: range.end,
                        }),
                        ..source.clone()
                    },
                },
            )
        })
        .collect();
    let failure = |name: &str, diagnostics: &mut Vec<Diagnostic>| {
        diagnostics.push(diagnostic(
            DiagnosticCode::JuliaMetadata,
            Severity::Error,
            format!("Project.toml field `{name}` has a missing or invalid static value."),
            fields.get(name).map_or(source, |field| &field.source),
        ));
    };
    let name = fields
        .get("name")
        .and_then(|field| field.value.as_str())
        .filter(|name| !name.trim().is_empty());
    let uuid = fields
        .get("uuid")
        .and_then(|field| field.value.as_str())
        .filter(|uuid| valid_uuid(uuid));
    if name.is_none() {
        failure("name", diagnostics);
    }
    if uuid.is_none() {
        failure("uuid", diagnostics);
    }
    let version = match fields.get("version") {
        Some(field) => match field
            .value
            .as_str()
            .filter(|value| semver::Version::parse(value).is_ok())
        {
            Some(value) => Some(value.to_owned()),
            None => {
                failure("version", diagnostics);
                return None;
            }
        },
        None => None,
    };
    let authors = match fields.get("authors") {
        Some(field) => match field.value.as_array().and_then(|values| {
            values
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        }) {
            Some(authors) => authors,
            None => {
                failure("authors", diagnostics);
                return None;
            }
        },
        None => Vec::new(),
    };
    let mut tables = Vec::new();
    for key in ["deps", "compat"] {
        let table = match fields.get(key) {
            None => BTreeMap::new(),
            Some(field) => match field.value.as_object().and_then(|table| {
                table
                    .iter()
                    .map(|(name, value)| {
                        value.as_str().map(|value| (name.clone(), value.to_owned()))
                    })
                    .collect::<Option<BTreeMap<_, _>>>()
            }) {
                Some(table) if key != "deps" || table.values().all(|value| valid_uuid(value)) => {
                    table
                }
                _ => {
                    failure(key, diagnostics);
                    return None;
                }
            },
        };
        tables.push(table);
    }
    let name = name?.to_owned();
    let uuid = uuid?.to_owned();
    Some(JuliaMetadata {
        name,
        uuid,
        version,
        authors,
        dependencies: tables.remove(0),
        compatibility: tables.remove(0),
        fields,
        source: source.clone(),
    })
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

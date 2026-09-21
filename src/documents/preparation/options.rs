//! Literal option values and precedence; declarations remain in authored IR.

use std::collections::BTreeMap;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::execution::{
    EffectiveCellOptions, EffectiveOption, ExecutionDefaults, OptionOrigin, OutputVisibility,
};
use crate::ir::{CellOption, CellOptionSource, CodeCell, MetadataEntry, MetadataValue};

use super::error;
use super::scalars::Value;

pub(super) fn document_defaults(
    source: &str,
    metadata: Option<&MetadataValue>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (ExecutionDefaults, bool) {
    let mut options = EffectiveCellOptions::default();
    let mut veto = false;
    let Some(MetadataValue::Mapping { entries, .. }) = metadata else {
        // The authority validator already diagnoses a non-mapping document.
        return (options.execution, veto);
    };
    duplicate_metadata(entries, diagnostics);
    for entry in entries {
        match entry.key.value.as_str() {
            "title" => {
                if Value::Yaml(&entry.value, source)
                    .string()
                    .is_none_or(|s| s.trim().is_empty())
                {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidQmdMetadata,
                        "Document title must be a nonempty string.",
                        entry.span,
                    ));
                }
            }
            "audience" => {
                let value = Value::Yaml(&entry.value, source);
                if value.string().is_none() && value.strings().is_none() {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidQmdMetadata,
                        "Document audience must be a string or a sequence of strings.",
                        entry.span,
                    ));
                }
            }
            "execute" => match &entry.value {
                MetadataValue::Mapping { entries, .. } => {
                    duplicate_metadata(entries, diagnostics);
                    for option in entries {
                        let key = option.key.value.as_str();
                        if matches!(key, "mode" | "engine" | "kernel" | "<<") {
                            continue;
                        }
                        if !inheritable(key) {
                            diagnostics.push(error(
                                DiagnosticCode::UnsupportedQmdMetadata,
                                format!("Unsupported document execution option `{key}`."),
                                option.span,
                            ));
                        } else if let Some(value) =
                            parse_option(key, Value::Yaml(&option.value, source))
                        {
                            apply(
                                &mut options,
                                value,
                                OptionOrigin::Document { span: option.span },
                            );
                        } else {
                            diagnostics.push(error(
                                DiagnosticCode::InvalidQmdMetadata,
                                format!("Invalid value for document execution option `{key}`."),
                                option.span,
                            ));
                        }
                    }
                }
                value => veto |= Value::Yaml(value, source).boolean() == Some(false),
            },
            // Authority checks own these diagnostics, including their grouping.
            "jupyter" | "engine" | "kernel" | "execution" | "<<" => {}
            key => diagnostics.push(error(
                DiagnosticCode::UnsupportedQmdMetadata,
                format!("Unsupported document metadata `{key}`."),
                entry.span,
            )),
        }
    }
    (options.execution, veto)
}

fn duplicate_metadata(entries: &[MetadataEntry], diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = BTreeMap::new();
    for entry in entries {
        if let Some(first) = seen.insert(&entry.key.value, entry.span) {
            let mut diagnostic = error(
                DiagnosticCode::InvalidQmdMetadata,
                "Document metadata keys must be unique within each mapping.",
                first,
            );
            diagnostic.related_spans.push(entry.span);
            diagnostics.push(diagnostic);
        }
    }
}

pub(super) fn cell_options(
    source: &str,
    cell: &CodeCell,
    defaults: &ExecutionDefaults,
    diagnostics: &mut Vec<Diagnostic>,
) -> EffectiveCellOptions {
    let mut options = EffectiveCellOptions {
        execution: defaults.clone(),
        ..EffectiveCellOptions::default()
    };
    for tier in [CellOptionSource::InlineInfo, CellOptionSource::HashpipeYaml] {
        let mut declarations = BTreeMap::<&str, Vec<&CellOption>>::new();
        for declaration in cell.options.iter().filter(|d| d.source == tier) {
            let Some(key) = declaration.canonical_key.as_deref() else {
                diagnostics.push(error(
                    DiagnosticCode::InvalidCellOption,
                    "A cell option must have a scalar key and a value.",
                    declaration.span,
                ));
                continue;
            };
            declarations.entry(key).or_default().push(declaration);
        }
        for (key, group) in declarations {
            if group.len() > 1 {
                let mut diagnostic = error(
                    DiagnosticCode::AmbiguousCellOption,
                    format!(
                        "Cell option `{key}` has multiple declarations in the same precedence tier."
                    ),
                    group[0].span,
                );
                diagnostic
                    .related_spans
                    .extend(group[1..].iter().map(|d| d.span));
                diagnostics.push(diagnostic);
            }
            for declaration in &group {
                if !supported(key) {
                    diagnostics.push(error(
                        DiagnosticCode::UnsupportedCellOption,
                        format!("Unsupported cell option `{key}`."),
                        declaration.span,
                    ));
                    continue;
                }
                let value = match tier {
                    CellOptionSource::InlineInfo => {
                        // Panache omits quote tokens from raw_value; recover them
                        // from the authored declaration to distinguish booleans.
                        let raw = source[declaration.span.start..declaration.span.end]
                            .split_once('=')
                            .map(|(_, value)| value.trim())
                            .unwrap_or("");
                        Value::Inline(raw)
                    }
                    CellOptionSource::HashpipeYaml => match &declaration.value {
                        Some(value) => Value::Yaml(value, source),
                        None => Value::Inline(""),
                    },
                };
                if let Some(value) = parse_option(key, value) {
                    if group.len() == 1 {
                        let origin = match tier {
                            CellOptionSource::InlineInfo => OptionOrigin::Inline {
                                span: declaration.span,
                            },
                            CellOptionSource::HashpipeYaml => OptionOrigin::Hashpipe {
                                span: declaration.span,
                            },
                        };
                        apply(&mut options, value, origin);
                    }
                } else {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidCellOption,
                        format!("Invalid value for cell option `{key}`."),
                        declaration.span,
                    ));
                }
            }
        }
    }
    options
}

fn inheritable(key: &str) -> bool {
    matches!(key, "eval" | "echo" | "output" | "include" | "error")
}

fn supported(key: &str) -> bool {
    inheritable(key) || matches!(key, "label" | "fig-alt" | "fig-cap" | "fig-subcap")
}

pub(super) fn valid_label(label: &str) -> bool {
    label
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

enum ParsedOption {
    Eval(bool),
    Echo(bool),
    Output(OutputVisibility),
    Include(bool),
    Error(bool),
    Label(String),
    FigAlt(String),
    FigCap(String),
    FigSubcap(Vec<String>),
}

fn parse_option(key: &str, value: Value<'_>) -> Option<ParsedOption> {
    Some(match key {
        "eval" => ParsedOption::Eval(value.boolean()?),
        "echo" => ParsedOption::Echo(value.boolean()?),
        "include" => ParsedOption::Include(value.boolean()?),
        "error" => ParsedOption::Error(value.boolean()?),
        "output" => ParsedOption::Output(match value.boolean() {
            Some(true) => OutputVisibility::Show,
            Some(false) => OutputVisibility::Hide,
            None if value.string().as_deref() == Some("asis") => OutputVisibility::AsIs,
            None => return None,
        }),
        "label" => ParsedOption::Label(value.string().filter(|s| valid_label(s))?),
        "fig-alt" => ParsedOption::FigAlt(value.string()?),
        "fig-cap" => ParsedOption::FigCap(value.string()?),
        "fig-subcap" => ParsedOption::FigSubcap(value.strings()?),
        _ => return None,
    })
}

fn apply(options: &mut EffectiveCellOptions, value: ParsedOption, origin: OptionOrigin) {
    match value {
        ParsedOption::Eval(value) => options.execution.eval = EffectiveOption { value, origin },
        ParsedOption::Echo(value) => options.execution.echo = EffectiveOption { value, origin },
        ParsedOption::Output(value) => options.execution.output = EffectiveOption { value, origin },
        ParsedOption::Include(value) => {
            options.execution.include = EffectiveOption { value, origin }
        }
        ParsedOption::Error(value) => options.execution.error = EffectiveOption { value, origin },
        ParsedOption::Label(value) => {
            options.label = EffectiveOption {
                value: Some(value),
                origin,
            }
        }
        ParsedOption::FigAlt(value) => {
            options.fig_alt = EffectiveOption {
                value: Some(value),
                origin,
            }
        }
        ParsedOption::FigCap(value) => {
            options.fig_cap = EffectiveOption {
                value: Some(value),
                origin,
            }
        }
        ParsedOption::FigSubcap(value) => options.fig_subcap = EffectiveOption { value, origin },
    }
}

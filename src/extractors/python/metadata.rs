use pyproject_toml::{Project, PyProjectToml};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::SourceLocation;

use super::{PythonMetadata, diagnostic};

pub(super) fn parse(
    text: &str,
    source: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
    selected_grammar: &mut Option<String>,
) -> Option<PythonMetadata> {
    let project = match PyProjectToml::new(text) {
        Ok(metadata) => match metadata.project {
            Some(project) => project,
            None => {
                diagnostics.push(diagnostic(
                    DiagnosticCode::PythonMetadata,
                    Severity::Error,
                    "Static Python metadata requires a [project] table.",
                    source,
                ));
                return None;
            }
        },
        Err(error) => {
            let mut source = source.clone();
            source.span = error.span().map(|range| crate::ir::SourceSpan {
                start: range.start,
                end: range.end,
            });
            diagnostics.push(diagnostic(
                DiagnosticCode::PythonMetadata,
                Severity::Error,
                format!("Invalid Python metadata: {}", error.message()),
                &source,
            ));
            return None;
        }
    };
    let mut valid = true;
    for field in project.dynamic.as_deref().unwrap_or_default() {
        let required = matches!(
            field.as_str(),
            "name" | "version" | "requires-python" | "dependencies" | "optional-dependencies"
        );
        diagnostics.push(diagnostic(DiagnosticCode::PythonDynamicMetadata,
            if required { Severity::Error } else { Severity::Warning },
            format!("Project field `{field}` is dynamic; static extraction cannot run a metadata backend."), source));
        valid &= !required;
    }
    if project.name.trim().is_empty() {
        valid = false;
        diagnostics.push(diagnostic(
            DiagnosticCode::PythonMetadata,
            Severity::Error,
            "The distribution name must not be empty.",
            source,
        ));
    }
    if project.version.is_none()
        && !project
            .dynamic
            .as_ref()
            .is_some_and(|fields| fields.iter().any(|field| field == "version"))
    {
        valid = false;
        diagnostics.push(diagnostic(
            DiagnosticCode::PythonMetadata,
            Severity::Error,
            "Static Python metadata requires a project version.",
            source,
        ));
    }
    let target_version = grammar(&project);
    if !project
        .dynamic
        .as_ref()
        .is_some_and(|fields| fields.iter().any(|field| field == "requires-python"))
    {
        *selected_grammar = target_version.clone();
    }
    if target_version.is_none() {
        valid = false;
        diagnostics.push(diagnostic(DiagnosticCode::PythonUnsupportedVersion, Severity::Error,
            "The declared Python requirement has no supported minimum grammar (supported: Python 3.7 through 3.14).", source));
    }
    if !valid {
        return None;
    }
    Some(PythonMetadata {
        name: project.name,
        version: project.version?.to_string(),
        description: project.description,
        requires_python: project.requires_python.as_ref().map(ToString::to_string),
        dependencies: project
            .dependencies
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect(),
        optional_dependencies: project
            .optional_dependencies
            .unwrap_or_default()
            .into_iter()
            .map(|(name, values)| (name, values.iter().map(ToString::to_string).collect()))
            .collect(),
        dynamic: project.dynamic.unwrap_or_default(),
        target_version: target_version?,
        source: source.clone(),
    })
}

fn grammar(project: &Project) -> Option<String> {
    let Some(requirement) = &project.requires_python else {
        // Absence is explicit: use the oldest supported grammar, never Ruff's
        // changing default. Provenance records this choice for consumers.
        return Some("3.7".into());
    };
    // Test release boundaries rather than only x.y.0: >=3.11.2 still selects
    // Python 3.11 syntax. PEP 440 comparison remains owned by the pinned parser.
    let mut patches = vec![0, 1];
    for specifier in requirement.iter() {
        let patch = specifier
            .version()
            .release()
            .get(2)
            .copied()
            .unwrap_or_default();
        patches.push(patch);
        patches.push(patch.saturating_add(1));
    }
    patches.sort_unstable();
    patches.dedup();
    for major in 2..=3 {
        for minor in 0..=14 {
            if patches.iter().any(|patch| {
                let version = format!("{major}.{minor}.{patch}")
                    .parse()
                    .expect("numeric version");
                requirement.contains(&version)
            }) {
                return (major == 3 && minor >= 7).then(|| format!("{major}.{minor}"));
            }
        }
    }
    None
}

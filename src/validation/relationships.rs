use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticEntity, Severity};
use crate::ir::{PackageReference, Workspace};

pub(super) fn validate(workspace: &Workspace) -> Vec<Diagnostic> {
    workspace
        .relationships
        .iter()
        .enumerate()
        .filter_map(|(index, relationship)| {
            let PackageReference::Workspace { package } = &relationship.to else {
                return None;
            };
            let constraint = relationship.version_constraint.as_deref()?;
            let package = workspace.packages.get(package)?;
            let result = package
                .version
                .as_deref()
                .and_then(|version| matches_constraint(&package.ecosystem, version, constraint));
            let (code, severity, message) = match result {
                Some(true) => return None,
                Some(false) => (
                    DiagnosticCode::IncompatiblePackageRelationship,
                    Severity::Error,
                    format!(
                        "Package `{}` version `{}` does not satisfy `{constraint}`.",
                        package.name,
                        package.version.as_deref().unwrap()
                    ),
                ),
                None => (
                    DiagnosticCode::IndeterminatePackageRelationship,
                    Severity::Warning,
                    format!(
                        "Cannot determine whether package `{}` satisfies `{constraint}`.",
                        package.name
                    ),
                ),
            };
            let mut diagnostic = Diagnostic::new(code, severity, message)
                .with_entity(DiagnosticEntity::Relationship { index });
            diagnostic.source = relationship
                .provenance
                .iter()
                .find_map(|p| p.source.clone());
            Some(diagnostic)
        })
        .collect()
}

fn matches_constraint(ecosystem: &str, version: &str, requirement: &str) -> Option<bool> {
    // Caret constraints in workspace declarations explicitly request SemVer,
    // including the cross-ecosystem acceptance fixtures.
    if ecosystem == "cargo" || requirement.trim_start().starts_with('^') {
        return Some(
            semver::VersionReq::parse(requirement)
                .ok()?
                .matches(&semver::Version::parse(version).ok()?),
        );
    }
    match ecosystem {
        "python" => Some(
            requirement
                .parse::<pep440_rs::VersionSpecifiers>()
                .ok()?
                .contains(&version.parse().ok()?),
        ),
        "r" => {
            let version = numeric_version(version)?;
            let mut matches = true;
            for comparator in requirement.split(',') {
                let comparator = comparator.trim();
                let operator = [">=", "<=", "==", "!=", ">", "<", "="]
                    .into_iter()
                    .find(|operator| comparator.starts_with(operator))?;
                let mut expected = numeric_version(comparator[operator.len()..].trim())?;
                let mut actual = version.clone();
                let width = actual.len().max(expected.len());
                actual.resize(width, 0);
                expected.resize(width, 0);
                matches &= match operator {
                    ">=" => actual >= expected,
                    "<=" => actual <= expected,
                    "==" | "=" => actual == expected,
                    "!=" => actual != expected,
                    ">" => actual > expected,
                    "<" => actual < expected,
                    _ => unreachable!(),
                };
            }
            Some(matches)
        }
        _ => None,
    }
}

fn numeric_version(value: &str) -> Option<Vec<u64>> {
    value
        .split(['.', '-'])
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constraints_follow_the_selected_syntax_without_guessing_unknown_versions() {
        for (ecosystem, version, constraint, expected) in [
            ("r", "1.8-1", ">= 1.8.0, < 2.0", Some(true)),
            ("r", "1.8.1", ">= 2.0", Some(false)),
            ("r", "1.8", "== 1.8.0", Some(true)),
            ("r", "1.8.0", "^2.0", Some(false)),
            ("r", "1.8-1", "^1.8", None),
            ("python", "1.9rc1", ">=1.9", Some(false)),
            ("python", "1.9.post1", ">=1.9, <2", Some(true)),
            ("python", "1.9", "~=1.8", Some(true)),
            ("unknown", "1.9.0", ">=1.0", None),
        ] {
            assert_eq!(matches_constraint(ecosystem, version, constraint), expected);
        }
    }
}

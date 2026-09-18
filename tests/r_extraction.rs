mod support;

use diplodocus::configuration::load_configuration;
use diplodocus::diagnostics::Severity;
use diplodocus::extractors::r::{RExtraction, extract_target};
use diplodocus::ir::{
    Block, Item, ItemLanguageData, ProvenanceActivity, RDeclaration, RGenericReference,
};
use diplodocus::paths::resolve_workspace_paths;

fn extract(workspace: &support::TestWorkspace) -> RExtraction {
    let path = workspace.path().join("workspace/diplodocus.toml");
    let config = load_configuration(&path).unwrap();
    let paths = resolve_workspace_paths(&path, &config).unwrap();
    let package = paths.packages.iter().find(|p| p.id == "rfoo").unwrap();
    extract_target(
        &paths.repositories[package.repository_index],
        package,
        &package.targets[0],
    )
}

fn small(source: &str, namespace: &str, rd: &str) -> support::TestWorkspace {
    let workspace = support::acceptance_workspace();
    for file in [
        "R/fit.R",
        "R/metrics.R",
        "R/experimental.R",
        "man/fit.Rd",
        "man/foo_model.Rd",
        "man/mean_squared_error.Rd",
        "man/experimental_summary.Rd",
    ] {
        workspace.remove(format!("r/{file}"));
    }
    workspace.write("r/R/api.R", source);
    workspace.write("r/NAMESPACE", namespace);
    if !rd.is_empty() {
        workspace.write("r/man/api.Rd", rd);
    }
    workspace
}

fn topic(name: &str, usage: &str) -> String {
    format!(
        "\\name{{{name}}}\\alias{{{name}}}\\title{{Example}}\\usage{{{usage}}}\\description{{Example prose.}}"
    )
}

fn codes(result: &RExtraction) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn item<'a>(result: &'a RExtraction, name: &str) -> &'a Item {
    result
        .items
        .values()
        .find(|i| i.qualified_name == name)
        .unwrap()
}

fn normalize_versions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(tools) = object.get_mut("tools").and_then(|v| v.as_object_mut()) {
                for name in ["diplodocus", "r"] {
                    if let Some(version) = tools.get_mut(name) {
                        assert_eq!(version, env!("CARGO_PKG_VERSION"));
                        *version = serde_json::json!("[DIPLODOCUS_VERSION]");
                    }
                }
            }
            for child in object.values_mut() {
                normalize_versions(child);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                normalize_versions(child);
            }
        }
        _ => {}
    }
}

#[test]
fn complete_r_extraction_matches_portable_acceptance_ir() {
    let workspace = support::acceptance_workspace();
    let result = extract(&workspace);
    assert_eq!(result, extract(&support::acceptance_workspace()));
    assert_eq!(codes(&result), ["r-rd-source-attribution"; 4]);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning && d.span.is_none())
    );
    assert_eq!(result.items.len(), 7);
    assert_eq!(
        result
            .items
            .values()
            .filter(|i| i.documentation.is_some())
            .count(),
        7
    );
    let metadata = result.metadata.as_ref().unwrap();
    assert_eq!(metadata.name, "foo");
    assert_eq!(metadata.version, "1.8.0");
    assert_eq!(metadata.title, "Statistical Models with Foo");
    let Some(ItemLanguageData::R(generic)) = &item(&result, "fit").language_data else {
        panic!()
    };
    let RDeclaration::S3Generic {
        methods,
        dispatch_name,
        ..
    } = &generic.declaration
    else {
        panic!()
    };
    assert_eq!(dispatch_name, "fit");
    assert_eq!(methods.len(), 2);
    let Some(ItemLanguageData::R(method)) = &item(&result, "predict.foo_model").language_data
    else {
        panic!()
    };
    assert!(!method.exported);
    assert!(
        matches!(&method.declaration, RDeclaration::S3Method { generic: RGenericReference::External { package, name }, .. } if package == "stats" && name == "predict")
    );
    assert!(
        matches!(&item(&result, "foo_model").language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::Constructor { classes } if classes == &["foo_model"]))
    );
    let ProvenanceActivity::Extraction {
        inputs,
        parsers,
        capabilities,
        ..
    } = &result.provenance.activity
    else {
        panic!()
    };
    assert_eq!(inputs["r"].len(), 9);
    assert_eq!(parsers["arity-parser"].version, "0.6.0");
    assert_eq!(parsers["rd-source"].version, "0.4.0");
    assert_eq!(capabilities.len(), 7);
    let bytes = serde_json::to_string(&result).unwrap();
    assert!(!bytes.contains(workspace.path().to_str().unwrap()));
    assert!(!bytes.contains("code-cell"));
    let mut golden = serde_json::to_value(result).unwrap();
    normalize_versions(&mut golden);
    support::assert_json_golden(&golden, "milestone-five/acceptance.json");
}

#[test]
fn shared_topics_tailor_usage_and_arguments_but_preserve_prose() {
    let result = extract(&support::acceptance_workspace());
    for name in ["fit", "fit.default", "fit.foo_model"] {
        let doc = item(&result, name).documentation.as_ref().unwrap();
        let usage = doc
            .document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::CodeBlock { source, .. } => Some(source),
                _ => None,
            })
            .unwrap();
        assert!(usage.starts_with(&format!("{name}(")), "{usage}");
        assert_eq!(
            usage.matches("tolerance").count(),
            usize::from(name != "fit")
        );
        let rendered = serde_json::to_string(&doc.document).unwrap();
        assert!(rendered.contains("Journal of Fixtures"));
        assert!(doc.source_location.as_ref().unwrap().span.is_none());
    }
}

#[test]
fn dynamic_rd_preserves_surrounding_content_and_one_placeholder() {
    let result = extract(&support::acceptance_case("unsupported-rd"));
    assert_eq!(result.items.len(), 7);
    assert_eq!(
        codes(&result)
            .iter()
            .filter(|c| **c == "unsupported-rd")
            .count(),
        1
    );
    assert_eq!(result.diagnostics.len(), 5);
    let doc = item(&result, "experimental_summary")
        .documentation
        .as_ref()
        .unwrap();
    let json = serde_json::to_string(&doc.document).unwrap();
    assert!(json.contains("unsupported"));
    assert!(json.contains("Compute three summary"));
    support::assert_json_golden(
        &result.diagnostics,
        "milestone-five/dynamic-diagnostics.json",
    );
}

#[test]
fn metadata_rejects_recovery_duplicates_and_partial_constraints() {
    for metadata in [
        "Package foo\nVersion: 1.0\nTitle: Test\n",
        "Package: foo\nVersion: 1.0\n",
        "Package: foo\nPackage: bar\nVersion: 1.0\nTitle: Test\n",
        "Package: foo\nVersion: bad\nTitle: Test\n",
        "Package: foo\nVersion: 1.0\nTitle: Test\nImports: stats (=> 4.0)\n",
        "Package: foo\nVersion: 1.0\nTitle: Test\nImports: stats (>= 4.0, broken)\n",
        "Package: foo\nVersion: 1.0\nTitle: Test\nImports: stats (>= 4.0\n",
    ] {
        let workspace = small("f <- function(x) x", "export(f)", &topic("f", "f(x)"));
        workspace.write("r/DESCRIPTION", metadata);
        let result = extract(&workspace);
        assert!(result.metadata.is_none(), "{metadata}");
        assert!(
            codes(&result).contains(&"r-metadata"),
            "{metadata}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn namespace_uncertainty_never_publishes_recovered_exports() {
    for namespace in [
        "if (getRversion() > '4.0') export(f) else export(g)",
        "futureDirective(f)\nexport(f)",
        "exportPattern('.*')",
        "export(f",
        "exportClasses(f)",
    ] {
        let result = extract(&small(
            "f <- function(x) x\ng <- function(x) x",
            namespace,
            "",
        ));
        assert!(result.items.is_empty(), "{namespace}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }
}

#[test]
fn source_recovery_and_dynamic_definitions_are_not_authoritative() {
    for source in [
        "f <- function(x) x\ng <-",
        "assign('f', function(x) x)",
        "if (TRUE) f <- function(x) x",
        "f <- function(x) x\nf <- function(y) y",
    ] {
        let result = extract(&small(source, "export(f)", &topic("f", "f(x)")));
        assert!(result.items.is_empty(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }
}

#[test]
fn aliases_and_explicit_method_bindings_share_canonical_entities() {
    let source = "f <- function(x, ...) UseMethod('f')\nimpl <- function(x, ...) x\nalias <- f\nprivate <- function() 1";
    let rd = "\\name{f}\\alias{f}\\alias{alias}\\alias{impl}\\title{Test}\\usage{f(x, ...)\n\\method{f}{default}(x, ...)}\\description{Shared}";
    let result = extract(&small(
        source,
        "export(f, alias)\nS3method(f, default, impl)",
        rd,
    ));
    assert_eq!(codes(&result), ["r-rd-source-attribution"]);
    assert_eq!(result.items.len(), 2);
    assert!(
        item(&result, "f")
            .aliases
            .iter()
            .any(|a| a.qualified_name == "alias")
    );
    assert!(item(&result, "impl").documentation.is_some());
}

#[test]
fn private_and_nested_definitions_do_not_become_public_generics() {
    let source = "f <- function(x) { hidden <- function(x) UseMethod('hidden'); x }\nprivate <- function() 1";
    let result = extract(&small(source, "export(f)", &topic("f", "f(x)")));
    assert_eq!(result.items.len(), 1);
    assert!(
        matches!(&item(&result, "f").language_data, Some(ItemLanguageData::R(data)) if data.declaration == RDeclaration::Function)
    );
}

#[test]
fn qualified_s3_registration_uses_the_local_method_binding() {
    let result = extract(&small(
        "predict.foo <- function(object, ...) object",
        "S3method(stats::predict, foo)",
        &topic("predict.foo", "\\method{predict}{foo}(object, ...)"),
    ));
    assert_eq!(codes(&result), ["r-rd-source-attribution"]);
    let method = item(&result, "predict.foo");
    assert!(
        matches!(&method.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Method { generic: RGenericReference::External { package, name }, .. } if package == "stats" && name == "predict"))
    );
}

#[test]
fn missing_and_conflicting_documentation_is_diagnosed() {
    let missing = extract(&small("f <- function(x) x", "export(f)", ""));
    assert!(codes(&missing).contains(&"r-missing-documented-alias"));
    let conflict = extract(&small(
        "f <- function(x) x",
        "export(f)",
        &topic("f", "f(y)"),
    ));
    assert!(codes(&conflict).contains(&"r-conflicting-surface"));
    let workspace = small("f <- function(x) x", "export(f)", &topic("f", "f(x)"));
    workspace.write("r/man/duplicate.Rd", topic("f", "f(x)"));
    let duplicate = extract(&workspace);
    assert!(codes(&duplicate).contains(&"r-conflicting-surface"));
}

#[test]
fn rd_errors_preserve_evidence_without_first_wins_semantics() {
    for (extra, code) in [
        ("\\description{Duplicate}", "r-rd-information-loss"),
        ("\\unknown{payload}", "r-rd-syntax"),
    ] {
        let result = extract(&small(
            "f <- function(x) x",
            "export(f)",
            &(topic("f", "f(x)") + extra),
        ));
        assert!(codes(&result).contains(&code), "{:?}", result.diagnostics);
    }
    let workspace = small("f <- function(x) x", "export(f)", "");
    workspace.write("r/man/api.Rd", [0xff, 0]);
    assert!(codes(&extract(&workspace)).contains(&"r-rd-syntax"));
}

#[test]
fn source_moves_and_comments_preserve_semantic_ids() {
    let workspace = support::acceptance_workspace();
    let first = extract(&workspace);
    let source = workspace.read("r/R/fit.R");
    workspace.remove("r/R/fit.R");
    workspace.write("r/R/renamed.R", format!("# moved\n{source}"));
    assert_eq!(
        first.items.keys().collect::<Vec<_>>(),
        extract(&workspace).items.keys().collect::<Vec<_>>()
    );
}

#[test]
fn extraction_does_not_run_package_code_or_documentation() {
    let workspace = support::acceptance_workspace();
    workspace.write("r/R/traps.R", "stop('must not source')\n.onLoad <- function(...) stop('must not load')\n.onAttach <- function(...) stop('must not attach')\n");
    let first = extract(&workspace);
    assert_eq!(first.items, extract(&support::acceptance_workspace()).items);
    assert!(!workspace.path().join("site").exists());
    let result = extract(&small(
        "f <- function(x = stop('must not evaluate')) x",
        "export(f)",
        &topic("f", "f(x = stop('must not evaluate'))"),
    ));
    assert_eq!(codes(&result), ["r-rd-source-attribution"]);
}

#[cfg(unix)]
#[test]
fn symlink_escapes_are_rejected_before_reading() {
    let workspace = small("f <- function(x) x", "export(f)", "");
    let outside = support::TestWorkspace::new();
    outside.write("outside.R", "secret <- function() 1");
    std::os::unix::fs::symlink(
        outside.path().join("outside.R"),
        workspace.path().join("r/R/escape.R"),
    )
    .unwrap();
    let result = extract(&workspace);
    assert!(codes(&result).contains(&"source-path-outside-boundary"));
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains(outside.path().to_str().unwrap())
    );
}

#[test]
fn rd_markup_and_document_relative_fallbacks_match_a_golden() {
    let rd = r"\name{f}\alias{f}\alias{friendly}\title{An \emph{example}}
\usage{f(x, ..., choice = 'a')}
\arguments{\item{x}{A \strong{value}.}\item{...}{More values.}\item{choice}{A choice.}}
\description{First paragraph with \code{x}, \url{https://example.org}, and \link{f}.

Second paragraph.}
\details{\itemize{\item One\item Two with \emph{markup}.}\describe{\item{Label}{Description.}}}
\examples{\dontrun{stop('inert')}\donttest{f(1)}}";
    let result = extract(&small(
        "f <- function(x, ..., choice = \"a\") x",
        "export(f)",
        rd,
    ));
    assert_eq!(codes(&result), ["r-rd-source-attribution"]);
    let doc = item(&result, "f").documentation.as_ref().unwrap();
    let value = serde_json::to_value(&doc.document).unwrap();
    assert_eq!(
        doc.document
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Paragraph { .. }))
            .count(),
        2
    );
    assert!(value.to_string().contains("emphasis"));
    assert!(value.to_string().contains("semantic-reference"));
    support::assert_json_golden(&value, "r-components/markup.json");
    assert!(
        item(&result, "f")
            .aliases
            .iter()
            .any(|a| a.qualified_name == "friendly")
    );
}

#[test]
fn rd_usage_is_attributed_to_its_actual_r_parser() {
    let result = extract(&small(
        "f <- function(x) x",
        "export(f)",
        &topic("f", "f(x)"),
    ));
    let ProvenanceActivity::Extraction {
        inputs, parsers, ..
    } = result.provenance.activity
    else {
        panic!()
    };
    assert!(
        inputs["r"][&"man/api.Rd".try_into().unwrap()]
            .parsers
            .contains("arity-parser")
    );
    assert_eq!(parsers["arity-parser"].settings["grammar:man/api.Rd"], "r");
}

#[test]
fn conflicting_method_registrations_do_not_choose_a_winner() {
    let result = extract(&small(
        "f <- function(x, ...) UseMethod('f')\ng <- function(x, ...) UseMethod('g')\nimpl <- function(x, ...) x",
        "export(f, g)\nS3method(f, default, impl)\nS3method(g, default, impl)",
        "",
    ));
    assert!(codes(&result).contains(&"r-conflicting-surface"));
    assert!(!result.items.values().any(|i| i.qualified_name == "impl"));
}

#[test]
fn dynamic_and_shadowed_dispatch_is_not_misclassified() {
    for source in [
        "f <- function(x) { UseMethod <- function(...) x; UseMethod('f') }",
        "f <- function(x) { assign('UseMethod', function(...) x); UseMethod('f') }",
        "f <- function(x, binding) { assign(binding, function(...) x); UseMethod('f') }",
        "f <- function(x) UseMethod(paste0('f'))",
        "UseMethod <- function(...) 1\nf <- function(x) UseMethod('f')",
    ] {
        let result = extract(&small(source, "export(f)", ""));
        assert!(!result.items.values().any(|item| matches!(&item.language_data, Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::S3Generic { .. }))));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }
}

#[test]
fn unsupported_usage_is_visible_and_unknown_calls_are_not_discarded() {
    let rd = topic("f", "f(x)\nghost(y)");
    let result = extract(&small("f <- function(x) x", "export(f)", &rd));
    assert!(codes(&result).contains(&"r-unresolved-definition"));
    let rd = topic("f", "f(x)\n\\Sexpr{stop('inert')}");
    let result = extract(&small("f <- function(x) x", "export(f)", &rd));
    assert!(codes(&result).contains(&"unsupported-rd"));
    assert!(
        serde_json::to_string(&item(&result, "f").documentation)
            .unwrap()
            .contains("unsupported")
    );
}

#[test]
fn defaults_and_non_syntactic_names_preserve_structure_and_ranges() {
    let source = "`strange name` <- function(`arg name`, ..., option = NULL, formula = ~ x + y) 1";
    let rd = topic(
        "strange name",
        "`strange name`(`arg name`, ..., option = NULL, formula = ~ x + y)",
    );
    let result = extract(&small(source, "export(`strange name`)", &rd));
    assert_eq!(codes(&result), ["r-rd-source-attribution"]);
    support::assert_json_golden(
        &item(&result, "strange name").signatures,
        "r-components/signature.json",
    );
}

#[test]
fn repeated_dispatch_coordinates_are_conflicts_even_with_different_bindings() {
    for (source, namespace, remaining) in [
        (
            "f <- function(x) UseMethod('f')\na <- function(x) x\nb <- function(x) x",
            "export(f)\nS3method(f, default, a)\nS3method(f, default, b)",
            1,
        ),
        (
            "f <- function(x) UseMethod('f')\nalias <- f\na <- function(x) x\nb <- function(x) x",
            "export(f, alias)\nS3method(f, default, a)\nS3method(alias, default, b)",
            1,
        ),
        (
            "a <- function(x) x\nb <- function(x) x",
            "importFrom(stats, predict)\nS3method(predict, default, a)\nS3method(stats::predict, default, b)",
            0,
        ),
    ] {
        let result = extract(&small(source, namespace, ""));
        assert!(
            codes(&result).contains(&"r-conflicting-surface"),
            "{namespace}"
        );
        assert_eq!(result.items.len(), remaining);
    }
}

#[test]
fn invalid_formals_and_conditional_rebindings_do_not_publish_old_definitions() {
    for source in [
        "f <- function(x, x) x",
        "f <- function(x) x\nif (flag) f <- function(y) y",
        "f <- function(x) x\nassign('f', function(y) y)",
    ] {
        let result = extract(&small(source, "export(f)", ""));
        assert!(result.items.is_empty(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }
}

#[test]
fn grouped_argument_names_are_tailored_and_unknown_arguments_are_diagnosed() {
    let rd = r"\name{f}\alias{f}\alias{g}\title{Shared}
\usage{f(x)
g(y)}
\arguments{\item{x, y}{Shared description.}\item{obsolete}{Retained raw text.}}
\description{Shared prose.}";
    let result = extract(&small(
        "f <- function(x) x\ng <- function(y) y",
        "export(f, g)",
        rd,
    ));
    assert!(codes(&result).contains(&"r-rd-information-loss"));
    for (name, expected) in [("f", "x"), ("g", "y")] {
        let doc = item(&result, name).documentation.as_ref().unwrap();
        let entries = doc
            .document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::List { items, .. } => Some(items),
                _ => None,
            })
            .unwrap();
        let Block::Paragraph { inlines, .. } = &entries[0].blocks[0] else {
            panic!()
        };
        assert!(
            matches!(&inlines[0], diplodocus::ir::Inline::Code { value, .. } if value == expected)
        );
        assert!(
            serde_json::to_string(&entries[0])
                .unwrap()
                .contains("Shared description.")
        );
        assert!(
            doc.raw_source
                .as_ref()
                .unwrap()
                .contains("Retained raw text.")
        );
    }
}

#[test]
fn invalid_explicit_input_boundaries_are_diagnosed() {
    let workspace = support::acceptance_workspace();
    let path = workspace.path().join("workspace/diplodocus.toml");
    let config = load_configuration(&path).unwrap();
    let paths = resolve_workspace_paths(&path, &config).unwrap();
    let mut package = paths
        .packages
        .iter()
        .find(|p| p.id == "rfoo")
        .unwrap()
        .clone();
    package.metadata_path = workspace.path().join("outside/DESCRIPTION");
    workspace.write("outside/DESCRIPTION", "private boundary sentinel");
    let result = extract_target(
        &paths.repositories[package.repository_index],
        &package,
        &package.targets[0],
    );
    assert!(result.metadata.is_none());
    assert!(codes(&result).contains(&"source-path-outside-boundary"));
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("private boundary sentinel"));
    assert!(!serialized.contains(workspace.path().to_str().unwrap()));
}

#[test]
#[cfg(unix)]
fn input_paths_that_cannot_be_serialized_are_diagnosed() {
    use std::os::unix::ffi::OsStrExt;
    let workspace = support::acceptance_workspace();
    let path = workspace
        .path()
        .join("r/R")
        .join(std::ffi::OsStr::from_bytes(b"invalid-\xff.R"));
    std::fs::write(path, "private <- function() 1").unwrap();
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    );
}

#[test]
fn extraction_succeeds_without_a_runtime_on_path() {
    const PROBE: &str = "DIPLODOCUS_R_STATIC_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let result = extract(&support::acceptance_workspace());
        assert_eq!(result.items.len(), 7);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.severity == Severity::Warning)
        );
        return;
    }
    let empty_path = support::TestWorkspace::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "extraction_succeeds_without_a_runtime_on_path"])
        .env(PROBE, "1")
        .env("PATH", empty_path.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn all_acceptance_cases_have_their_registered_r_diagnostics() {
    for case in support::acceptance_registry().cases {
        let workspace = support::materialize_case(&case);
        let path = workspace.path().join(&case.config);
        let config = load_configuration(&path).unwrap();
        let paths = resolve_workspace_paths(&path, &config).unwrap();
        let mut actual = vec![];
        for package in &paths.packages {
            for target in &package.targets {
                if !config
                    .packages
                    .iter()
                    .find(|p| p.id == package.id)
                    .unwrap()
                    .targets
                    .iter()
                    .any(|t| t.id == target.id && t.extractor == "r")
                {
                    continue;
                }
                let result = extract_target(
                    &paths.repositories[package.repository_index],
                    package,
                    target,
                );
                for diagnostic in result.diagnostics {
                    let diplodocus::diagnostics::DiagnosticSource::Repository { repository, path } =
                        diagnostic.source.unwrap()
                    else {
                        panic!()
                    };
                    actual.push((
                        diagnostic.code.as_str().to_owned(),
                        format!("{repository}/{}", path.as_str()),
                    ));
                }
            }
        }
        let mut expected: Vec<_> = case
            .check
            .iter()
            .filter(|d| d.code.starts_with("r-") || d.code == "unsupported-rd")
            .map(|d| (d.code.clone(), d.source.clone()))
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected, "{}", case.id);
    }
}

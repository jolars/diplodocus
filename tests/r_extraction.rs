mod support;

use diplodocus::diagnostics::Severity;
use diplodocus::ir::{
    Block, DocumentContainerKind, Inline, Item, ItemLanguageData, PackageMetadata,
    ProvenanceActivity, RDeclaration, RGenericReference, Signature,
};

use support::{ApiDiscovery, discover_r_api as extract};

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

fn codes(result: &ApiDiscovery) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn item<'a>(result: &'a ApiDiscovery, name: &str) -> &'a Item {
    result
        .items
        .values()
        .find(|i| i.qualified_name == name)
        .unwrap_or_else(|| panic!("missing {name}: {:?}", result.diagnostics))
}

#[test]
fn eulerr_datasets_are_documented_without_source_bindings_or_exports() {
    let workspace = small("", "", "");
    support::add_r_datasets(&workspace);
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "{:?}",
        result.diagnostics
    );
    for name in ["fruits", "organisms", "pain", "plants"] {
        let dataset = item(&result, name);
        assert_eq!(dataset.kind, diplodocus::ir::ItemKind::Constant);
        assert!(matches!(&dataset.language_data,
            Some(ItemLanguageData::R(data)) if !data.exported && matches!(data.declaration, RDeclaration::Dataset)));
        assert!(matches!(
            dataset.signatures[0].signature,
            diplodocus::ir::Signature::Value {
                annotation: None,
                value: None
            }
        ));
        let doc = dataset.documentation.as_ref().unwrap();
        assert_eq!(
            doc.raw_source.as_deref(),
            Some(workspace.read(format!("r/man/{name}.Rd")).as_str())
        );
        let headings: Vec<_> = doc
            .document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Heading { inlines, .. } => Some(serde_json::to_string(inlines).unwrap()),
                _ => None,
            })
            .collect();
        assert!(headings.iter().any(|heading| heading.contains("Format")));
        assert!(headings.iter().any(|heading| heading.contains("Usage")));
        if name != "fruits" {
            assert!(headings.iter().any(|heading| heading.contains("Source")));
        }
        assert!(
            doc.document
                .blocks
                .iter()
                .any(|block| matches!(block, Block::CodeBlock { source, .. } if source == name))
        );
        assert!(
            dataset
                .aliases
                .iter()
                .any(|alias| alias.qualified_name == name)
        );
        let id = diplodocus::ir::SemanticIdentity::r_dataset(name).unwrap();
        assert!(result.items.contains_key(id.item_id()));
        assert_ne!(
            id,
            diplodocus::ir::SemanticIdentity::r_function(name).unwrap()
        );
    }
    let unsupported: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "unsupported-rd")
        .map(|d| d.message.as_str())
        .collect();
    assert!(unsupported.is_empty(), "{unsupported:?}");
    assert!(item(&result, "pain").documentation.as_ref().unwrap().document.blocks.iter()
        .any(|block| matches!(block, Block::Heading { inlines, .. } if serde_json::to_string(inlines).unwrap().contains("Disclaimer"))));
}

#[test]
fn dataset_aliases_non_syntactic_usage_and_explicit_exports_are_preserved() {
    let rd = r"\docType{data}\name{strange data}\alias{strange data}\alias{extra}
\title{Dataset}\usage{`strange data`
extra}\format{A list.}\source{A survey.}
\description{Documented data.}\keyword{internal}";
    let workspace = small("`strange data` <- list()", "export(`strange data`)", rd);
    let original = extract(&workspace);
    assert_eq!(codes(&original), ["r-rd-source-attribution"]);
    let dataset = item(&original, "strange data");
    assert!(matches!(&dataset.language_data,
        Some(ItemLanguageData::R(data)) if data.exported && data.internal));
    assert!(!dataset.discoverable());
    assert_eq!(dataset.provenance.len(), 2);
    assert!(dataset.documentation.as_ref().unwrap().document.blocks.iter().any(|block|
        matches!(block, Block::CodeBlock { source, .. } if source == "`strange data`")));
    assert!(
        dataset
            .aliases
            .iter()
            .any(|alias| alias.qualified_name == "extra")
    );
    assert!(
        dataset
            .documentation
            .as_ref()
            .unwrap()
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeBlock { source, .. } if source == "extra"))
    );
    workspace.write("r/man/moved.Rd", format!("% A relocated topic.\n{rd}"));
    workspace.remove("r/man/api.Rd");
    let moved = extract(&workspace);
    assert_eq!(
        original.items.keys().collect::<Vec<_>>(),
        moved.items.keys().collect::<Vec<_>>()
    );
}

#[test]
fn dataset_usage_and_sections_reject_unsupported_or_ambiguous_shapes() {
    for usage in [
        "fruits()",
        "fruits[1]",
        "fruits <- 1",
        "'fruits'",
        "NULL",
        "ghost",
    ] {
        let rd = format!(r"\docType{{data}}{}", topic("fruits", usage));
        let result = extract(&small("", "", &rd));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{usage}"
        );
    }
    for section in [
        r"\docType{data}",
        r"\format{Another format.}",
        r"\source{Another source.}",
    ] {
        let rd = format!(
            r"\docType{{data}}{}\format{{A list.}}\source{{A survey.}}{section}",
            topic("fruits", "fruits")
        );
        let result = extract(&small("", "", &rd));
        assert!(
            codes(&result).contains(&"r-rd-information-loss"),
            "{section}"
        );
        assert!(result.items.is_empty());
    }
    let rd = format!(r"\docType{{data}}{}", topic("fruits", "fruits"));
    let workspace = small("", "", &rd);
    workspace.write("r/man/duplicate.Rd", &rd);
    let duplicate = extract(&workspace);
    assert!(
        duplicate
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        duplicate.diagnostics
    );
    let result = extract(&small("fruits <- list()", "exportPattern('fruits')", &rd));
    assert!(
        matches!(&item(&result, "fruits").language_data, Some(ItemLanguageData::R(data)) if data.exported)
    );
    let result = extract(&small("", "S3method(print, fruits, fruits)", &rd));
    assert!(
        codes(&result).contains(&"r-metadata"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn base_s3_generics_resolve_without_explicit_imports() {
    let result = extract(&small(
        "print.foo <- function(x, ...) x",
        "S3method(print, foo)",
        &topic("print.foo", "\\method{print}{foo}(x, ...)"),
    ));
    assert!(!codes(&result).contains(&"r-unresolved-definition"));
    let Some(ItemLanguageData::R(data)) = &item(&result, "print.foo").language_data else {
        panic!()
    };
    assert!(
        matches!(&data.declaration, RDeclaration::S3Method { generic: RGenericReference::External { package, name }, .. } if package == "base" && name == "print")
    );
}

#[test]
fn standard_s3_generics_resolve_from_defaults_and_whole_package_imports() {
    for (generic, package, namespace) in [
        ("coef", "stats", ""),
        ("coefficients", "stats", ""),
        ("fitted", "stats", ""),
        ("residuals", "stats", ""),
        ("plot", "base", ""),
        ("sort", "base", ""),
        ("as.data.frame", "base", ""),
        ("[", "base", ""),
        ("/", "base", ""),
        ("|", "base", ""),
        ("Ops", "base", ""),
        ("hist", "graphics", ""),
        ("head", "utils", ""),
        ("as.raster", "grDevices", ""),
        ("makeContent", "grid", "import(grid)"),
        ("makeContext", "grid", "import(grid)"),
        ("predict", "stats", "import(stats)"),
    ] {
        let binding = format!("{generic}.euler");
        let result = extract(&small(
            &format!("`{binding}` <- function(x, ...) stop('must not execute')"),
            &format!("{namespace}\nS3method(\"{generic}\", euler)"),
            &topic(&binding, &format!("\\method{{{generic}}}{{euler}}(x, ...)")),
        ));
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.severity != Severity::Error),
            "{generic}: {:?}",
            result.diagnostics
        );
        assert!(matches!(&item(&result, &binding).language_data,
            Some(ItemLanguageData::R(data)) if matches!(&data.declaration,
                RDeclaration::S3Method { generic: RGenericReference::External { package: actual, name }, .. }
                if actual == package && name == generic)));
    }
    let missing = extract(&small(
        "makeContent.foo <- function(x) x",
        "S3method(makeContent, foo)",
        "",
    ));
    assert!(
        missing
            .items
            .values()
            .all(|item| item.name != "makeContent.foo")
    );
}

#[test]
fn documented_helpers_and_grid_hooks_retain_internal_semantics() {
    let workspace = small(
        "setup_gpar <- function(default = list(), user = list(), n) stop('must not execute')\nundocumented <- function(x) x\nmakeContent.EulerGlyphs <- function(x) x",
        "import(grid)\nS3method(makeContent, EulerGlyphs)",
        &format!(
            "{}\\keyword{{internal}}",
            topic(
                "setup_gpar",
                "setup_gpar(default = list(), user = list(), n)"
            )
        ),
    );
    workspace.write(
        "r/man/hook.Rd",
        format!(
            "{}\\keyword{{internal}}",
            topic(
                "makeContent.EulerGlyphs",
                "\\method{makeContent}{EulerGlyphs}(x)"
            )
        ),
    );
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(result.items.len(), 2);
    for name in ["setup_gpar", "makeContent.EulerGlyphs"] {
        let helper = item(&result, name);
        assert!(!helper.discoverable());
        assert!(helper.documentation.is_some());
        assert!(!helper.signatures.is_empty());
        assert!(
            matches!(&helper.language_data, Some(ItemLanguageData::R(data)) if !data.exported && data.internal)
        );
    }
}

#[test]
fn internal_documentation_selects_the_loaded_private_binding() {
    let rd = format!("{}\\keyword{{internal}}", topic("helper", "helper(y)"));
    let result = extract(&small(
        "helper <- function(x) x\nif (TRUE) helper <- function(y) y",
        "",
        &rd,
    ));
    let item = item(&result, "helper");
    assert!(
        matches!(&item.language_data, Some(ItemLanguageData::R(data)) if data.internal && !data.exported)
    );
    assert!(
        matches!(&item.signatures[0].signature, Signature::Callable { parameters, .. } if parameters[0].name == "y")
    );
    let missing = extract(&small("", "", &rd));
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        missing.diagnostics
    );
}

#[test]
#[ignore = "Requires a prepared eulerr checkout via DIPLODOCUS_EULERR_PATH."]
fn eulerr_native_discovery_resolves_registered_generics_and_documented_helpers() {
    let checkout = std::path::PathBuf::from(
        std::env::var_os("DIPLODOCUS_EULERR_PATH")
            .expect("Set DIPLODOCUS_EULERR_PATH to a prepared eulerr checkout"),
    );
    let workspace = small("", "", "");
    let path = workspace.path().join("core/diplodocus.yaml");
    let mut config = diplodocus::configuration::load_configuration(&path).unwrap();
    let package = config.packages.iter_mut().find(|p| p.id == "rfoo").unwrap();
    package.r.get_or_insert_with(Default::default).documentation =
        diplodocus::configuration::RDocumentationMode::Rd;
    workspace.write(&path, serde_saphyr::to_string(&config).unwrap());
    let namespace = std::fs::read_to_string(checkout.join("NAMESPACE")).unwrap();
    workspace.write("r/NAMESPACE", &namespace);
    workspace.write(
        "r/DESCRIPTION",
        std::fs::read(checkout.join("DESCRIPTION")).unwrap(),
    );
    for directory in ["R", "man", "data", "inst"] {
        let path = checkout.join(directory);
        if !path.is_dir() {
            continue;
        }
        for file in support::files_under(&path) {
            workspace.write(
                std::path::Path::new("r").join(directory).join(&file),
                std::fs::read(path.join(&file)).unwrap(),
            );
        }
    }
    for library in ["src/eulerr.so", "src/eulerr.dll", "src/eulerr.dylib"] {
        let path = checkout.join(library);
        if path.is_file() {
            workspace.write(format!("r/{library}"), std::fs::read(path).unwrap());
        }
    }
    let result = extract(&workspace);
    let failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
    for name in ["EULER_PANEL_PAD_PT", "eulerr_deprecated", "eulerr_options"] {
        assert!(item(&result, name).documentation.is_some(), "{name}");
    }
    let method_count = result.items.values().filter(|item| matches!(&item.language_data, Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::S3Method { .. }))).count();
    assert_eq!(
        method_count,
        namespace
            .lines()
            .filter(|line| line.starts_with("S3method("))
            .count(),
        "{:?}",
        result.diagnostics
    );
    for (binding, package, generic) in [
        ("coef.euler", "stats", "coef"),
        ("fitted.euler", "stats", "fitted"),
        ("residuals.euler", "stats", "residuals"),
        ("plot.euler", "base", "plot"),
        ("/.eulergram", "base", "/"),
        ("|.eulergram", "base", "|"),
        ("makeContent.EulerGlyphs", "grid", "makeContent"),
        ("makeContext.EulerPanel", "grid", "makeContext"),
    ] {
        assert!(
            matches!(&item(&result, binding).language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration,
            RDeclaration::S3Method { generic: RGenericReference::External { package: actual_package, name }, .. }
            if actual_package == package && name == generic))
        );
    }
    for binding in ["|.eulergram", "/.eulergram"] {
        let method = item(&result, binding);
        let Some(ItemLanguageData::R(data)) = &method.language_data else {
            panic!()
        };
        let topic = data.help_topic.as_ref().unwrap();
        assert_eq!(topic.name, "eulergram-compose");
        assert_eq!(topic.members.len(), 2);
        let document = method.documentation.as_ref().unwrap();
        for usage in ["`|.eulergram`(e1, e2)", "`/.eulergram`(e1, e2)"] {
            assert!(
                document.document.blocks.iter().any(|block| matches!(block,
                Block::CodeBlock { source, .. } if source == usage)),
                "{binding}: {usage}"
            );
        }
        assert!(
            result.items[&topic.primary.item]
                .aliases
                .iter()
                .any(|alias| alias.qualified_name == "eulergram-compose")
        );
        assert!(!result.diagnostics.iter().any(|diagnostic| diagnostic.severity == Severity::Error &&
            matches!(&diagnostic.source, Some(diplodocus::diagnostics::DiagnosticSource::Repository { path, .. }) if path.as_str() == "man/eulergram-compose.Rd")), "{:?}", result.diagnostics);
    }
    for name in [
        "setup_gpar",
        "setup_grobs",
        "is_integer",
        "is_real",
        "makeContent.EulerGlyphs",
        "makeContent.EulerTags",
        "makeContent.EulerSetLabels",
        "makeContext.EulerPanel",
    ] {
        let helper = item(&result, name);
        assert!(!helper.discoverable(), "{name}");
        assert!(helper.documentation.is_some(), "{name}");
        assert!(
            matches!(&helper.language_data, Some(ItemLanguageData::R(data)) if !data.exported && data.internal)
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains(name)),
            "{name}: {:?}",
            result.diagnostics
        );
    }
    eprintln!(
        "Resolved {method_count} eulerr S3 registrations; {} total items.",
        result.items.len()
    );
    for name in ["fruits", "organisms", "pain", "plants"] {
        let dataset = item(&result, name);
        assert!(matches!(&dataset.language_data,
            Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::Dataset)));
        assert!(
            dataset
                .documentation
                .as_ref()
                .unwrap()
                .document
                .blocks
                .iter()
                .any(|block| matches!(block, Block::CodeBlock { source, .. } if source == name))
        );
    }
}

#[test]
fn package_rd_topic_is_documented_without_a_function() {
    let rd = r"\name{rfoo-package}\alias{rfoo}\alias{rfoo-package}\title{R Foo}\description{Package description.}\docType{package}\keyword{internal}";
    let result = extract(&small("", "", rd));
    assert!(!codes(&result).contains(&"r-unresolved-definition"));
    assert!(!codes(&result).contains(&"unsupported-rd"));
    let package = item(&result, "rfoo-package");
    assert!(matches!(package.kind, diplodocus::ir::ItemKind::Namespace));
    assert!(package.signatures.is_empty());
    assert!(package.documentation.is_some());
    assert!(package.discoverable());
    assert!(
        package
            .aliases
            .iter()
            .any(|alias| alias.qualified_name == "rfoo")
    );
}

#[test]
fn native_r_discovery_retains_portable_acceptance_ir() {
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
    let Some(PackageMetadata::R(metadata)) = &result.metadata else {
        panic!("missing R package metadata")
    };
    let metadata = &metadata.description;
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
    } = &result.extraction().activity
    else {
        panic!()
    };
    assert_eq!(inputs["r"].len(), 9);
    assert_eq!(
        parsers["arity-parser"].version,
        env!("DIPLODOCUS_ARITY_VERSION")
    );
    assert_eq!(parsers["rd-source"].version, "0.5.0");
    assert!(capabilities.contains("r.namespace.native"));
    let bytes = serde_json::to_string(&result).unwrap();
    assert!(!bytes.contains(workspace.path().to_str().unwrap()));
    assert!(!bytes.contains("code-cell"));
}

#[test]
fn shared_topics_preserve_complete_usage_arguments_and_prose() {
    let result = extract(&support::acceptance_workspace());
    for name in ["fit", "fit.default", "fit.foo_model"] {
        let doc = item(&result, name).documentation.as_ref().unwrap();
        let usages: Vec<_> = doc
            .document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::CodeBlock { source, .. } => Some(source.as_str()),
                _ => None,
            })
            .collect();
        assert!(usages.iter().any(|usage| usage.starts_with("fit(x, ...")));
        assert!(usages.iter().any(|usage| usage.starts_with("fit.default(")));
        assert!(
            usages
                .iter()
                .any(|usage| usage.starts_with("fit.foo_model("))
        );
        let data = match &item(&result, name).language_data {
            Some(ItemLanguageData::R(data)) => data,
            _ => unreachable!(),
        };
        let topic = data.help_topic.as_ref().unwrap();
        assert_eq!(topic.name, "fit");
        assert_eq!(topic.members.len(), 3);
        assert_eq!(result.items[&topic.primary.item].qualified_name, "fit");
        let blocks = serde_json::to_string(&doc.document.blocks).unwrap();
        for argument in ["solver", "tolerance", "features", "target"] {
            assert!(blocks.contains(argument), "{name}: {argument}");
        }
        assert_eq!(doc, item(&result, "fit").documentation.as_ref().unwrap());
        let rendered = serde_json::to_string(&doc.document).unwrap();
        assert!(rendered.contains("Journal of Fixtures"));
        assert!(doc.source_location.as_ref().unwrap().span.is_none());
    }
}

#[test]
fn shared_topic_aliases_have_one_primary_without_merging_declarations() {
    let workspace = support::shared_r_workspace();
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "{:?}",
        result.diagnostics
    );
    for (alias, primary, members) in [
        (
            "model-fitting",
            "fit",
            vec!["fit", "fit.default", "fit.model"],
        ),
        (
            "eulergram-compose",
            "|.eulergram",
            vec!["/.eulergram", "|.eulergram"],
        ),
    ] {
        let owners: Vec<_> = result
            .items
            .values()
            .filter(|item| item.aliases.iter().any(|a| a.qualified_name == alias))
            .collect();
        assert_eq!(owners.len(), 1, "{alias}");
        assert_eq!(owners[0].qualified_name, primary);
        let Some(ItemLanguageData::R(data)) = &owners[0].language_data else {
            panic!()
        };
        let topic = data.help_topic.as_ref().unwrap();
        assert_eq!(result.items[&topic.primary.item].qualified_name, primary);
        assert_eq!(topic.members.len(), members.len());
        for name in members {
            let member = item(&result, name);
            assert_eq!(member.documentation, owners[0].documentation);
            assert_eq!(member.signatures.len(), 1);
        }
    }
    let doc = item(&result, "fit").documentation.as_ref().unwrap();
    assert!(
        serde_json::to_string(&doc.document)
            .unwrap()
            .contains("weights")
    );
    let usages: Vec<_> = item(&result, "|.eulergram")
        .documentation
        .as_ref()
        .unwrap()
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::CodeBlock { source, .. } if source.ends_with("(e1, e2)") => {
                Some(source.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(usages, ["`|.eulergram`(e1, e2)", "`/.eulergram`(e1, e2)"]);
    let relocated = support::shared_r_workspace();
    assert_eq!(result, extract(&relocated));
}

#[test]
fn shared_topic_alias_collisions_still_fail() {
    let workspace = support::shared_r_workspace();
    workspace.write(
        "r/man/fit.Rd",
        workspace
            .read("r/man/fit.Rd")
            .replace("\\alias{model-fitting}", "\\alias{eulergram-compose}"),
    );
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error
                && d.code.as_str() == "r-conflicting-surface"
                && d.message.contains("eulergram-compose")),
        "{:?}",
        result.diagnostics
    );
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
fn unsupported_rd_nodes_retain_source_once_in_the_enclosing_document() {
    let rd = r"\name{f}\alias{f}\title{Example}\usage{f(x)}
\description{Before \Sexpr{1} and \out{x}, then \Sexpr{2} after.}
\examples{\Sexpr{stop('must not execute')}}";
    let result = extract(&small("f <- function(x) x", "export(f)", rd));
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    );
    assert_eq!(
        codes(&result)
            .iter()
            .filter(|code| **code == "unsupported-rd")
            .count(),
        4
    );
    let doc = item(&result, "f").documentation.as_ref().unwrap();
    assert_eq!(doc.raw_source.as_deref(), Some(rd));

    let inline_kinds: Vec<_> = doc
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { inlines, .. } => Some(inlines),
            _ => None,
        })
        .flatten()
        .filter_map(|inline| match inline {
            diplodocus::ir::Inline::Unsupported {
                source_kind,
                raw,
                span,
            } => {
                assert!(raw.is_empty());
                assert_eq!(span.start, 0);
                assert_eq!(span.end, rd.len());
                Some(source_kind.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(inline_kinds, ["rd-\\Sexpr", "rd-\\out", "rd-\\Sexpr"]);
    let block_kinds: Vec<_> = doc
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Unsupported {
                source_kind, raw, ..
            } => {
                assert!(raw.is_empty());
                Some(source_kind.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(block_kinds, ["rd-examples"]);
    let document = serde_json::to_string(&doc.document).unwrap();
    assert!(document.contains("Before "));
    assert!(document.contains(" after."));
    assert!(!document.contains("must not execute"));
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
    assert!(value.to_string().contains("@ref f"));
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
    } = &result.extraction().activity
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
    for (source, namespace) in [
        (
            "f <- function(x) UseMethod('f')\na <- function(x) x\nb <- function(x) x",
            "export(f)\nS3method(f, default, a)\nS3method(f, default, b)",
        ),
        (
            "f <- function(x) UseMethod('f')\nalias <- f\na <- function(x) x\nb <- function(x) x",
            "export(f, alias)\nS3method(f, default, a)\nS3method(alias, default, b)",
        ),
        (
            "a <- function(x) x\nb <- function(x) x",
            "importFrom(stats, predict)\nS3method(predict, default, a)\nS3method(stats::predict, default, b)",
        ),
    ] {
        let result = extract(&small(source, namespace, ""));
        assert!(
            codes(&result).contains(&"r-conflicting-surface"),
            "{namespace}"
        );
        assert!(result.items.is_empty());
    }
}

#[test]
fn invalid_package_code_does_not_produce_a_public_surface() {
    for source in [
        "f <- function(x, x) x",
        "f <- function(x) x\nif (flag) f <- function(y) y",
    ] {
        let result = extract(&small(source, "export(f)", ""));
        assert!(result.items.is_empty(), "{source}");
        assert!(
            codes(&result).contains(&"r-metadata"),
            "{:?}",
            result.diagnostics
        );
    }
}

#[test]
fn rd_ellipsis_argument_labels_match_variadic_formals() {
    for (label, expected) in [
        ("...", "..."),
        (r"\dots", "..."),
        (r"\ldots", "..."),
        (r"{\dots}", "..."),
        (r"x, \dots", "x, ..."),
    ] {
        let rd = format!(
            r"\name{{f}}\alias{{f}}\title{{Variadic arguments}}
\usage{{f(x, ...)}}
\arguments{{\item{{{label}}}{{Additional arguments.}}}}
\description{{A variadic function.}}"
        );
        let result = extract(&small("f <- function(x, ...) x", "export(f)", &rd));
        assert_eq!(codes(&result), ["r-rd-source-attribution"], "{label}");
        let function = item(&result, "f");
        let Signature::Callable { parameters, .. } = &function.signatures[0].signature else {
            panic!("expected a callable signature");
        };
        assert_eq!(parameters[1].name, "...");
        let doc = function.documentation.as_ref().unwrap();
        assert_eq!(doc.raw_source.as_deref(), Some(rd.as_str()));
        let entries = doc
            .document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Container {
                    kind: DocumentContainerKind::DefinitionList,
                    blocks,
                    ..
                } => Some(blocks),
                _ => None,
            })
            .unwrap();
        let Block::Container { kind, blocks, .. } = &entries[0] else {
            panic!("expected an argument term");
        };
        assert_eq!(*kind, DocumentContainerKind::DefinitionTerm);
        let Block::Paragraph { inlines, .. } = &blocks[0] else {
            panic!("expected an argument label");
        };
        assert!(matches!(inlines.as_slice(), [Inline::Code { value, .. }] if value == expected));
        assert!(
            serde_json::to_string(&entries[1])
                .unwrap()
                .contains("Additional arguments.")
        );
    }
}

#[test]
fn qualpal_s3_method_topics_accept_rd_ellipsis_arguments() {
    let workspace = small(
        "plot.qualpal <- function(x, ...) x\npairs.qualpal <- function(x, ...) x\nprint.qualpal <- function(x, ...) x\n",
        "S3method(plot, qualpal)\nS3method(pairs, qualpal)\nS3method(print, qualpal)\n",
        "",
    );
    for generic in ["plot", "pairs", "print"] {
        workspace.write(
            format!("r/man/{generic}.qualpal.Rd"),
            format!(
                r"\name{{{generic}.qualpal}}\alias{{{generic}.qualpal}}\title{{Palette method}}
\usage{{\method{{{generic}}}{{qualpal}}(x, ...)}}
\arguments{{\item{{x}}{{A palette.}}\item{{\dots}}{{Additional method arguments.}}}}
\description{{A method for qualitative color palettes.}}"
            ),
        );
    }
    let result = extract(&workspace);
    assert_eq!(codes(&result), ["r-rd-source-attribution"; 3]);
    for generic in ["plot", "pairs", "print"] {
        let method = item(&result, &format!("{generic}.qualpal"));
        assert!(matches!(&method.language_data,
            Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::S3Method { .. })));
        assert!(method.documentation.is_some());
    }
}

#[test]
fn rd_ellipsis_argument_labels_require_a_matching_formal() {
    let rd = topic("f", "f(x)") + r"\arguments{\item{\dots}{Additional arguments.}}";
    let result = extract(&small("f <- function(x) x", "export(f)", &rd));
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_str() == "r-rd-information-loss"
            && diagnostic.severity == Severity::Error
            && diagnostic
                .message
                .contains("Rd argument `...` does not match")
    }));
}

#[test]
fn grouped_argument_names_are_preserved_and_unknown_arguments_are_diagnosed() {
    let rd = r"\name{f}\alias{f}\alias{g}\title{Shared}
\usage{f(x)
g(y)}
\arguments{\item{x, y}{Shared description.}\item{obsolete}{Retained raw text.}}
\description{Shared prose.}";
    let source = "f <- function(x) x\ng <- function(y) y";
    let invalid = extract(&small(source, "export(f, g)", rd));
    assert!(codes(&invalid).contains(&"r-rd-information-loss"));
    let result = extract(&small(
        source,
        "export(f, g)",
        &rd.replace(r"\item{obsolete}{Retained raw text.}", ""),
    ));
    for name in ["f", "g"] {
        let doc = item(&result, name).documentation.as_ref().unwrap();
        let entries = doc
            .document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Container {
                    kind: diplodocus::ir::DocumentContainerKind::DefinitionList,
                    blocks,
                    ..
                } => Some(blocks),
                _ => None,
            })
            .unwrap();
        let Block::Container { kind, blocks, .. } = &entries[0] else {
            panic!()
        };
        assert_eq!(*kind, diplodocus::ir::DocumentContainerKind::DefinitionTerm);
        let Block::Paragraph { inlines, .. } = &blocks[0] else {
            panic!()
        };
        assert!(
            matches!(&inlines[0], diplodocus::ir::Inline::Code { value, .. } if value == "x, y")
        );
        assert!(
            serde_json::to_string(&entries[1])
                .unwrap()
                .contains("Shared description.")
        );
    }
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
fn extraction_reports_missing_r_on_path() {
    const PROBE: &str = "DIPLODOCUS_R_DISCOVERY_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let result = extract(&support::acceptance_workspace());
        assert!(result.items.is_empty());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Native R discovery requires R")),
            "{:?}",
            result.diagnostics
        );
        return;
    }
    let empty_path = support::TestWorkspace::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "extraction_reports_missing_r_on_path",
            "--nocapture",
        ])
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
fn documented_constants_retain_values_aliases_and_source() {
    let workspace = small(
        "PAD <- 3\nalias <- PAD\nprivate <- 9",
        "export(alias)",
        &format!(
            "{}\\alias{{alias}}\\keyword{{internal}}",
            topic("PAD", "PAD")
        ),
    );
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(result.items.len(), 1);
    let constant = item(&result, "PAD");
    assert!(matches!(constant.kind, diplodocus::ir::ItemKind::Constant));
    assert!(
        matches!(&constant.signatures[0].signature, diplodocus::ir::Signature::Value { value: Some(diplodocus::ir::SignatureExpression::Literal { text }), .. } if text == "3")
    );
    assert!(
        constant
            .aliases
            .iter()
            .any(|alias| alias.qualified_name == "alias")
    );
    assert_eq!(
        constant.source_location.as_ref().unwrap().path.as_str(),
        "R/api.R"
    );
    assert!(!constant.discoverable());
}

#[test]
fn deprecated_help_topics_preserve_aliases_without_inventing_functions() {
    let result = extract(&small(
        "",
        "",
        r"\name{rfoo_deprecated}\alias{rfoo_deprecated}\alias{old_api}\title{Deprecated functions}\description{Use newer APIs.}\keyword{internal}",
    ));
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        result.diagnostics
    );
    let help = item(&result, "rfoo_deprecated");
    assert!(help.signatures.is_empty());
    assert!(help.documentation.is_some());
    assert!(
        help.aliases
            .iter()
            .any(|alias| alias.qualified_name == "old_api")
    );
    assert!(!help.discoverable());
}

#[test]
fn standalone_deprecated_topics_preserve_arguments_without_callable_formals() {
    let original = include_str!("fixtures/qualpalr-help/autopal-deprecated.Rd");
    for name in ["autopal-deprecated", "autopal_deprecated"] {
        let rd = original.replace("autopal-deprecated", name);
        let result = extract(&small("", "", &rd));
        assert_eq!(codes(&result), ["r-rd-source-attribution"], "{name}");
        assert_eq!(result.items.len(), 1);
        let help = item(&result, name);
        assert!(matches!(&help.language_data,
            Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::DeprecatedTopic)));
        assert!(help.signatures.is_empty());
        assert!(!help.discoverable());
        let doc = help.documentation.as_ref().unwrap();
        assert_eq!(doc.raw_source.as_deref(), Some(rd.as_str()));
        let arguments = doc
            .document
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Container {
                    kind: DocumentContainerKind::DefinitionList,
                    blocks,
                    ..
                } => Some(blocks),
                _ => None,
            })
            .unwrap();
        let (arguments, remainder) = arguments.as_chunks::<2>();
        assert!(remainder.is_empty());
        assert_eq!(arguments.len(), 4);
        for (entry, (name, description)) in arguments.iter().zip([
            ("n", "Number of colors to generate."),
            (
                "colorspace",
                "specifying one of the predefined color spaces",
            ),
            ("cvd", "Color vision deficiency adaptation"),
            ("target", "Target color difference."),
        ]) {
            let Block::Container { kind, blocks, .. } = &entry[0] else {
                panic!("expected an argument term");
            };
            assert_eq!(*kind, DocumentContainerKind::DefinitionTerm);
            let Block::Paragraph { inlines, .. } = &blocks[0] else {
                panic!("expected an argument label");
            };
            assert!(matches!(inlines.as_slice(), [Inline::Code { value, .. }] if value == name));
            assert!(
                serde_json::to_string(&entry[1])
                    .unwrap()
                    .contains(description)
            );
        }
    }
}

#[test]
fn topics_without_usage_still_validate_maintained_function_arguments() {
    for name in ["old_api", "foo-deprecated", "foo_deprecated"] {
        let rd = format!(
            r"\name{{{name}}}\alias{{old_api}}\title{{Old API}}
\arguments{{\item{{x}}{{A value.}}\item{{obsolete}}{{An unknown argument.}}}}
\description{{Use another API.}}"
        );
        let result = extract(&small("old_api <- function(x) x", "export(old_api)", &rd));
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{name}: {:?}", result.diagnostics);
        assert_eq!(errors[0].code.as_str(), "r-rd-information-loss");
        assert!(
            errors[0]
                .message
                .contains("Rd argument `obsolete` does not match")
        );
        assert!(result.items.is_empty());
    }
}

#[test]
fn documented_constants_use_the_loaded_binding() {
    let rd = topic("PAD", "PAD");
    let result = extract(&small("PAD <- 3\nPAD <- 4", "", &rd));
    let constant = item(&result, "PAD");
    assert!(
        matches!(&constant.language_data, Some(ItemLanguageData::R(data)) if data.declaration == RDeclaration::Constant)
    );
    assert!(constant.documentation.is_some());
    let missing = extract(&small("", "", &rd));
    assert!(
        codes(&missing).contains(&"r-unresolved-definition"),
        "{:?}",
        missing.diagnostics
    );
}

#[test]
fn eulerr_constant_deprecated_and_options_topics_extract_without_content_loss() {
    let workspace = small(
        "EULER_PANEL_PAD_PT <- 3\neulerr_options <- function(...) NULL",
        "export(eulerr_options)",
        "",
    );
    for (name, text) in [
        (
            "EULER_PANEL_PAD_PT",
            include_str!("fixtures/r-help-topics/EULER_PANEL_PAD_PT.Rd"),
        ),
        (
            "eulerr_deprecated",
            include_str!("fixtures/r-help-topics/eulerr_deprecated.Rd"),
        ),
        (
            "eulerr_options",
            include_str!("fixtures/r-help-topics/eulerr_options.Rd"),
        ),
    ] {
        workspace.write(format!("r/man/{name}.Rd"), text);
    }
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error || d.code.as_str() == "unsupported-rd"),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(result.items.len(), 3);
    for name in ["EULER_PANEL_PAD_PT", "eulerr_deprecated", "eulerr_options"] {
        let doc = item(&result, name).documentation.as_ref().unwrap();
        assert_eq!(
            doc.raw_source.as_deref(),
            Some(workspace.read(format!("r/man/{name}.Rd")).as_str())
        );
    }
    let options = item(&result, "eulerr_options")
        .documentation
        .as_ref()
        .unwrap();
    let text = serde_json::to_string(&options.document).unwrap();
    for label in [
        "pointsize",
        "patterns",
        "annotations",
        "glyphs",
        "complement",
        "composition",
        "#'",
    ] {
        assert!(text.contains(label), "{label}");
    }
}

#[test]
fn deprecated_topics_attach_to_maintained_functions_when_available() {
    let result = extract(&small(
        "old_api <- function(x) x",
        "export(old_api)",
        r"\name{foo-deprecated}\alias{foo-deprecated}\alias{old_api}\title{Deprecated APIs}\description{Use another API.}",
    ));
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(result.items.len(), 1);
    let function = item(&result, "old_api");
    assert!(
        matches!(&function.language_data, Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::Function))
    );
    assert!(function.documentation.is_some());
    assert!(
        function
            .aliases
            .iter()
            .any(|alias| alias.qualified_name == "foo-deprecated")
    );
}

#[test]
fn constants_without_usage_are_documented_but_cannot_register_as_methods() {
    let rd = r"\name{PAD}\alias{PAD}\title{Padding}\description{Padding in points.}";
    let result = extract(&small("PAD <- 3", "", rd));
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    );
    assert!(item(&result, "PAD").documentation.is_some());
    let result = extract(&small("PAD <- 3", "S3method(print, PAD, PAD)", rd));
    assert!(codes(&result).contains(&"r-unresolved-definition"));
}

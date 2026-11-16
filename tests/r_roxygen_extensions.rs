mod support;

use std::process::{Command, Output};

use diplodocus::ir::{
    ItemLanguageData, ProvenanceActivity, RDeclaration, RGenericReference, Signature,
    SignatureExpression,
};
use diplodocus::snapshots::Snapshot;

fn workspace(settings: &str, source: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Roxygen extensions\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Roxygen extensions\n    slug: demo\n    ecosystem: r\n    repository: repo\n    path: package\n    metadata-path: DESCRIPTION\n{settings}    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n"));
    root.write("package/DESCRIPTION", "Package: roxyworkflow\nVersion: 0.1.0\nTitle: Roxygen Extensions Fixture\nDescription: Exercises native documentation extensions.\nLicense: MIT\nAuthor: Fixture Author\nMaintainer: Fixture Author <fixture@example.com>\nEncoding: UTF-8\nRoxygen: list(markdown = TRUE)\n");
    root.write("package/R/api.R", source);
    root
}

fn cli(root: &support::TestWorkspace, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("R_LIBS_USER", root.path().join("library"))
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
}

#[test]
fn native_exports_and_generated_s3_methods_keep_shared_help() {
    let source = r#"#' Work with a model
#'
#' Shared topic prose.
#' @param x A value.
#' @param ... Additional arguments.
#' @export
model <- function(x, ...) UseMethod("model")

#' @rdname model
#' @export
model.numeric <- local({ function(x, ...) x })

#' Return a generated value
#' @param x A value.
#' @export
generated <- local({ function(x = 1L) x })
"#;
    let root = workspace("", source);
    success(cli(&root, &["extract"]));
    let before = snapshot(&root);
    let package = &before.workspace().packages["demo"];
    let generic = package
        .items
        .values()
        .find(|item| item.name == "model")
        .unwrap();
    let method = package
        .items
        .values()
        .find(|item| item.name == "model.numeric")
        .unwrap();
    assert!(
        matches!(&generic.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Generic { methods, .. } if methods.len() == 1))
    );
    assert!(
        matches!(&method.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Method { generic: RGenericReference::Workspace { .. }, class, .. } if class == "numeric"))
    );
    assert_eq!(
        generic.documentation.as_ref().unwrap().raw_source,
        method.documentation.as_ref().unwrap().raw_source
    );
    assert!(
        root.read("package/NAMESPACE")
            .contains("S3method(model,numeric)")
    );
    let generated = package
        .items
        .values()
        .find(|item| item.name == "generated")
        .unwrap();
    assert!(matches!(&generated.language_data, Some(ItemLanguageData::R(data)) if data.exported));

    root.write(
        "package/R/api.R",
        source.replace("#' @export\ngenerated", "generated"),
    );
    success(cli(&root, &["extract"]));
    assert!(!root.read("package/NAMESPACE").contains("export(generated)"));
    let after = snapshot(&root);
    let generated = after.workspace().packages["demo"]
        .items
        .values()
        .find(|item| item.name == "generated")
        .unwrap();
    assert!(matches!(&generated.language_data, Some(ItemLanguageData::R(data)) if !data.exported));
}

#[test]
fn inherited_documentation_refreshes_after_the_parent_changes() {
    let source = r#"#' Parent operation
#'
#' Initial inherited prose.
#' @param x The inherited argument.
#' @export
parent <- function(x = 1L) x

#' Derived operation
#' @inherit parent
#' @export
derived <- local({ function(x = 1L) x })
"#;
    let root = workspace("", source);
    success(cli(&root, &["extract"]));
    assert!(
        root.read("package/man/derived.Rd")
            .contains("Initial inherited prose.")
    );
    assert!(
        root.read("package/man/derived.Rd")
            .contains("The inherited argument.")
    );
    root.write(
        "package/R/api.R",
        source.replace("Initial inherited prose.", "Changed inherited prose."),
    );
    success(cli(&root, &["extract"]));
    let topic = root.read("package/man/derived.Rd");
    assert!(topic.contains("Changed inherited prose."));
    assert!(!topic.contains("Initial inherited prose."));
    let loaded = snapshot(&root);
    let derived = loaded.workspace().packages["demo"]
        .items
        .values()
        .find(|item| item.name == "derived")
        .unwrap();
    assert_eq!(
        derived
            .documentation
            .as_ref()
            .unwrap()
            .raw_source
            .as_deref(),
        Some(topic.as_str())
    );
}

#[test]
fn maintained_numeric_defaults_keep_their_source_spellings() {
    let formals = "tolerance = 1e-8, real = 1.0, integer = 1L, empty = NULL, mask = 0xff, precise = 1.2345678901234567, imaginary = 1i, nested = list(z = 1i), callback = function(a = 1i, b) a, collision = list(quote(.__diplodocus_complex_literal_1_end), \".__diplodocus_complex_literal_2_end\", 1i)";
    let root = workspace(
        "    r:\n      documentation: rd\n",
        &format!("numeric_defaults <- function({formals}) tolerance\n"),
    );
    root.write("package/NAMESPACE", "export(numeric_defaults)\n");
    root.write("package/man/numeric_defaults.Rd", format!("\\name{{numeric_defaults}}\\alias{{numeric_defaults}}\\title{{Numeric defaults}}\\usage{{numeric_defaults({formals})}}\\description{{Maintained spellings survive native reflection.}}"));
    let config_path = root.path().join("diplodocus.yaml");
    let config = diplodocus::configuration::load_configuration(&config_path).unwrap();
    let paths = diplodocus::paths::resolve_workspace_paths(&config_path, &config).unwrap();
    let package = &paths.packages[0];
    let source = diplodocus::extractors::r::extract_target(
        &paths.repositories[package.repository_index],
        package,
        &package.targets[0],
    );
    let source_signature = source
        .items
        .values()
        .find(|item| item.name == "numeric_defaults")
        .unwrap()
        .signatures[0]
        .signature
        .clone();
    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    let item = loaded.workspace().packages["demo"]
        .items
        .values()
        .find(|item| item.name == "numeric_defaults")
        .unwrap();
    assert!(!item.signatures[0].sources.is_empty());
    assert_eq!(item.signatures[0].signature, source_signature);
    let Signature::Callable { parameters, .. } = &item.signatures[0].signature else {
        panic!()
    };
    for (parameter, expected) in parameters.iter().zip([
        "1e-8",
        "1.0",
        "1L",
        "NULL",
        "0xff",
        "1.2345678901234567",
        "1i",
    ]) {
        assert_eq!(
            parameter.default,
            Some(SignatureExpression::Literal {
                text: expected.into()
            })
        );
    }
}

#[test]
fn maintained_escaped_string_defaults_keep_their_source_spellings() {
    let formals = r#"unicode = "\u0041", wide = "\U00000041", braced = "\u{0041}", hex = "\x41", octal = "\101", controls = "\a\b\f\n\r\t\v", quote = '\"', slash = "\\", nested = list(x = "\u0041")"#;
    let root = workspace(
        "    r:\n      documentation: rd\n",
        &format!("string_defaults <- function({formals}) unicode\n"),
    );
    root.write("package/NAMESPACE", "export(string_defaults)\n");
    let usage_formals = formals.replace('\\', r"\\");
    root.write("package/man/string_defaults.Rd", format!("\\name{{string_defaults}}\\alias{{string_defaults}}\\title{{String defaults}}\\usage{{string_defaults({usage_formals})}}\\description{{Escaped defaults survive native reflection.}}"));

    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    let item = loaded.workspace().packages["demo"]
        .items
        .values()
        .find(|item| item.name == "string_defaults")
        .unwrap();
    assert!(!item.signatures[0].sources.is_empty());
    let Signature::Callable { parameters, .. } = &item.signatures[0].signature else {
        panic!()
    };
    assert_eq!(
        parameters[0].default,
        Some(SignatureExpression::Literal {
            text: r#""\u0041""#.into()
        })
    );

    root.write(
        "package/R/api.R",
        format!(
            "string_defaults <- function({}) unicode\n",
            formals.replacen(r#""\u0041""#, "\"B\"", 1)
        ),
    );
    let output = cli(&root, &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("r-conflicting-surface"));
}

#[test]
fn private_binding_failures_are_reported_only_when_selected() {
    let root = workspace(
        "    r:\n      documentation: rd\n",
        "f <- function(x) x\ndelayedAssign('unused', stop('not initialized'))\n",
    );
    root.write("package/NAMESPACE", "export(f)\n");
    let rd = "\\name{f}\\alias{f}\\title{Function}\\usage{f(x)}\\description{Working public API.}";
    root.write("package/man/f.Rd", rd);

    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    let items = &loaded.workspace().packages["demo"].items;
    assert_eq!(items.len(), 1);
    assert!(items.values().any(|item| item.name == "f"));

    root.write(
        "package/man/f.Rd",
        rd.replace("\\alias{f}", "\\alias{f}\\alias{unused}"),
    );
    let output = cli(&root, &["check"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unused"), "{stderr}");
    assert!(stderr.contains("not initialized"), "{stderr}");
}

#[test]
fn native_dispatch_respects_shadowed_calls_and_nested_closures() {
    let root = workspace(
        "    r:\n      documentation: rd\n",
        "UseMethod <- function(generic) generic\nordinary <- function(x) UseMethod('ordinary')\nqualified <- function(x) base::UseMethod('qualified')\nnested <- function(x) { hidden <- function(y) UseMethod('hidden'); x }\n",
    );
    root.write("package/NAMESPACE", "export(ordinary, qualified, nested)\n");
    for name in ["ordinary", "qualified", "nested"] {
        root.write(
            format!("package/man/{name}.Rd"),
            format!("\\name{{{name}}}\\alias{{{name}}}\\title{{{name}}}\\usage{{{name}(x)}}\\description{{Native dispatch evidence.}}"),
        );
    }
    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    let package = &loaded.workspace().packages["demo"];
    for name in ["ordinary", "nested"] {
        let item = package
            .items
            .values()
            .find(|item| item.name == name)
            .unwrap();
        assert!(
            matches!(&item.language_data, Some(ItemLanguageData::R(data)) if data.declaration == RDeclaration::Function)
        );
    }
    let qualified = package
        .items
        .values()
        .find(|item| item.name == "qualified")
        .unwrap();
    assert!(
        matches!(&qualified.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Generic { dispatch_name, .. } if dispatch_name == "qualified"))
    );
}

#[test]
fn namespace_load_rebindings_keep_source_aliases_as_distinct_items() {
    let root = workspace(
        "    r:\n      documentation: rd\n",
        "identity <- function(x) x\nalias <- identity\n.onLoad <- function(libname, pkgname) { replacement <- function(x) x; environment(replacement) <- asNamespace(pkgname); utils::assignInNamespace('alias', replacement, ns = pkgname) }\n",
    );
    root.write("package/NAMESPACE", "export(identity, alias)\n");
    root.write("package/man/identity.Rd", "\\name{identity}\\alias{identity}\\alias{alias}\\title{Operations}\\usage{identity(x)\nalias(x)}\\description{Distinct loaded functions.}");
    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    let items = &loaded.workspace().packages["demo"].items;
    assert_eq!(items.len(), 2);
    assert!(items.values().any(|item| item.name == "identity"));
    assert!(items.values().any(|item| item.name == "alias"));
}

#[test]
fn unsupported_class_systems_are_reported_only_when_selected() {
    let root = workspace(
        "    r:\n      documentation: rd\n",
        "methods::setGeneric('s4api', function(x) methods::standardGeneric('s4api'))\nordinary <- function(x) x\n",
    );
    root.write(
        "package/DESCRIPTION",
        format!("{}Imports: methods\n", root.read("package/DESCRIPTION")),
    );
    root.write(
        "package/NAMESPACE",
        "import(methods)\nexport(s4api, ordinary)\n",
    );
    root.write("package/man/ordinary.Rd", "\\name{ordinary}\\alias{ordinary}\\title{Ordinary function}\\usage{ordinary(x)}\\description{Supported native function.}");
    let output = cli(&root, &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported S4 class system"));
    root.write("package/NAMESPACE", "import(methods)\nexport(ordinary)\n");
    success(cli(&root, &["extract"]));
    let loaded = snapshot(&root);
    assert_eq!(loaded.workspace().packages["demo"].items.len(), 1);
}

#[test]
fn prepared_custom_roclet_uses_declared_inputs_and_outputs() {
    let root = workspace(
        "    r:\n      extra-inputs: [template.txt]\n      generated-outputs: [extension-marker.txt]\n",
        "#' Return a value\n#' @export\nanswer <- function() 42L\n",
    );
    root.write("package/DESCRIPTION", root.read("package/DESCRIPTION").replace("Roxygen: list(markdown = TRUE)", "Roxygen: list(markdown = TRUE, roclets = c('rd', 'namespace', 'roxyextension::marker_roclet'))"));
    root.write("package/template.txt", "First custom result.\n");
    root.write("extension/DESCRIPTION", "Package: roxyextension\nVersion: 0.1.0\nTitle: Test Roxygen Extension\nDescription: Supplies a custom roclet for acceptance tests.\nLicense: MIT\nAuthor: Fixture Author\nMaintainer: Fixture Author <fixture@example.com>\nImports: roxygen2\n");
    root.write("extension/NAMESPACE", "export(marker_roclet)\nimportFrom(roxygen2,roclet_process,roclet_output)\nS3method(roclet_process,roclet_marker)\nS3method(roclet_output,roclet_marker)\n");
    root.write(
        "extension/R/marker.R",
        r#"marker_roclet <- function() roxygen2::roclet("marker")

roclet_process.roclet_marker <- function(x, blocks, env, base_path) {
  readLines(file.path(base_path, "template.txt"), warn = FALSE)
}

roclet_output.roclet_marker <- function(x, results, base_path, ...) {
  path <- file.path(base_path, "extension-marker.txt")
  writeLines(results, path)
  path
}
"#,
    );
    std::fs::create_dir(root.path().join("library")).unwrap();
    // The caller prepares extension tooling; Diplodocus only invokes it.
    success(
        Command::new("R")
            .current_dir(root.path())
            .args([
                "CMD",
                "INSTALL",
                "--no-byte-compile",
                "--no-help",
                "--no-html",
                "--no-test-load",
                "-l",
            ])
            .arg(root.path().join("library"))
            .arg(root.path().join("extension"))
            .output()
            .unwrap(),
    );

    success(cli(&root, &["extract"]));
    assert_eq!(
        root.read("package/extension-marker.txt"),
        "First custom result.\n"
    );
    let before = snapshot(&root).canonical_export().unwrap();
    assert!(snapshot(&root).workspace().provenance.iter().any(|record| matches!(&record.activity, ProvenanceActivity::RDocumentation { roclets, inputs, outputs, .. } if roclets.iter().any(|roclet| roclet == "roxyextension::marker_roclet") && inputs.iter().any(|input| input.source.path.as_str() == "package/template.txt") && outputs.iter().any(|output| output.source.path.as_str() == "package/extension-marker.txt"))));
    root.write("package/template.txt", "Changed custom result.\n");
    success(cli(&root, &["extract"]));
    assert_eq!(
        root.read("package/extension-marker.txt"),
        "Changed custom result.\n"
    );
    assert_ne!(before, snapshot(&root).canonical_export().unwrap());
}

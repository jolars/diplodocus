mod support;

use diplodocus::configuration::load_configuration;
use diplodocus::diagnostics::Severity;
use diplodocus::extractors::julia::{JuliaExtraction, extract_target};
use diplodocus::ir::{Item, ItemKind, ItemLanguageData, JuliaCallableRole, JuliaDeclaration};
use diplodocus::paths::resolve_workspace_paths;

fn small(source: &str) -> support::TestWorkspace {
    let workspace = support::TestWorkspace::new();
    workspace.write(
        "diplodocus.toml",
        r#"
[project]
name = "Julia tests"
[[repository]]
id = "repo"
path = "."
[[package]]
id = "demo"
name = "Demo"
slug = "julia"
ecosystem = "julia"
repository = "repo"
path = "."
metadata-path = "Project.toml"
targets = [{ id = "api", extractor = "julia", path = "src/Demo.jl", role = "public-api" }]
"#,
    );
    workspace.write("Project.toml", "name = \"Demo\"\nuuid = \"12345678-1234-1234-1234-123456789abc\"\nversion = \"1.2.3\"\n[compat]\njulia = \"1.10, 1.11\"\n");
    workspace.write("src/Demo.jl", source);
    workspace
}

fn extract(workspace: &support::TestWorkspace) -> JuliaExtraction {
    let path = workspace.path().join("diplodocus.toml");
    let configuration = load_configuration(&path).unwrap();
    let paths = resolve_workspace_paths(&path, &configuration).unwrap();
    let package = &paths.packages[0];
    extract_target(&paths.repositories[0], package, &package.targets[0])
}

fn item<'a>(result: &'a JuliaExtraction, name: &str) -> &'a Item {
    result
        .items
        .values()
        .find(|item| item.qualified_name == name && item.kind != ItemKind::Method)
        .unwrap_or_else(|| panic!("missing {name}: {:?}", result.diagnostics))
}

fn methods(result: &JuliaExtraction) -> Vec<&str> {
    result
        .items
        .iter()
        .filter(|(_, item)| item.kind == ItemKind::Method)
        .map(|(id, _)| id.as_str())
        .collect()
}

#[test]
fn static_includes_preserve_modules_public_surface_and_method_docs() {
    let workspace = small(
        "module Demo\nexport fit\ninclude(\"fit.jl\")\nmodule Sub\npublic score\ninclude(\"score.jl\")\nend\nprivate(x) = x\nend\n",
    );
    workspace.write("src/fit.jl", "\"Fit integer data.\"\nfit(x::Int; verbose=false) = x\nfit(x::Float64) = x\n\"Documented helper.\"\nhelper(x) = x\n");
    workspace.write("src/score.jl", "score(x::Vector{T}) where {T<:Real} = x\n");
    let result = extract(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        result.metadata.as_ref().unwrap().version.as_deref(),
        Some("1.2.3")
    );
    assert_eq!(result.inputs.len(), 4);
    assert_eq!(methods(&result).len(), 4);
    assert!(item(&result, "Demo.helper").signatures.len() == 1);
    assert!(item(&result, "Demo.Sub.score").signatures.len() == 1);
    assert!(
        !result
            .items
            .values()
            .any(|i| i.qualified_name == "Demo.private")
    );
    let Some(ItemLanguageData::Julia(data)) = &item(&result, "Demo.fit").language_data else {
        panic!()
    };
    assert!(
        matches!(&data.declaration, JuliaDeclaration::Callable { role: JuliaCallableRole::Family { methods }, .. } if methods.len() == 2)
    );
    assert_eq!(
        result
            .items
            .values()
            .filter(|i| i.kind == ItemKind::Method && i.documentation.is_some())
            .count(),
        2
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains(workspace.path().to_str().unwrap())
    );
}

#[test]
fn dispatch_identity_ignores_names_keywords_defaults_returns_and_order() {
    let first = extract(&small(
        "module Demo\nexport fit\nfit(x::T; verbose=false)::T where {T<:Real} = x\nfit(x::String=\"\") = x\nend\n",
    ));
    let second = extract(&small(
        "module Demo\nexport fit\nfit(renamed::String=\"new\") = renamed\nfit(value::S; other=1)::Any where {S<:Real} = value\nend\n",
    ));
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
    assert_eq!(methods(&first), methods(&second));
    assert_eq!(methods(&first).len(), 2);
    let duplicate = extract(&small(
        "module Demo\nexport f\nf(x::Int; a=1)=x\nf(y::Int; b=2)=y\nend\n",
    ));
    assert!(
        duplicate
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-conflicting-surface")
    );
}

#[test]
fn types_fields_explicit_constructors_macros_constants_and_external_methods() {
    let result = extract(&small(
        r#"module Demo
export Model, AbstractModel, Token, LIMIT, @twice
abstract type AbstractModel end
primitive type Token 8 end
"A model."
mutable struct Model{T} <: AbstractModel
    "Stored value."
    value::T
    Model(x::T) where {T} = new{T}(x)
end
Model(x::Int, y::Int) = Model(x+y)
const LIMIT::Int = 10
"Repeat an expression."
macro twice(ex)
    :($ex + $ex)
end
"Display a model."
Base.show(io::IO, x::Model) = print(io, x.value)
end
"#,
    ));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(item(&result, "Demo.Model").kind, ItemKind::Type);
    assert_eq!(item(&result, "Demo.Model.value").kind, ItemKind::Field);
    assert_eq!(item(&result, "Demo.LIMIT").kind, ItemKind::Constant);
    assert!(item(&result, "Demo.@twice").documentation.is_some());
    assert!(item(&result, "Base.show").documentation.is_none());
    assert_eq!(methods(&result).len(), 3);
}

#[test]
fn include_and_dynamic_failures_are_visible_without_evaluation() {
    for (body, code) in [
        ("include(\"Demo.jl\")", "julia-include"),
        ("include(\"missing.jl\")", "julia-source-read"),
        ("include(joinpath(@__DIR__, \"other.jl\"))", "julia-include"),
        (
            "include(\"../../outside.jl\")",
            "source-path-outside-boundary",
        ),
        ("if true\nf(x)=x\nend", "julia-unsupported-surface"),
        ("@eval f(x)=x", "julia-unsupported-surface"),
    ] {
        let result = extract(&small(&format!("module Demo\nexport f\n{body}\nend\n")));
        assert!(
            result.diagnostics.iter().any(|d| d.code.as_str() == code),
            "{body}: {:?}",
            result.diagnostics
        );
    }
    let workspace = small(
        "module Demo\nexport f\nf(x)=x\nwrite(\"executed\", \"bad\")\nerror(\"do not execute\")\nend\n",
    );
    assert!(extract(&workspace).diagnostics.is_empty());
    assert!(!workspace.path().join("executed").exists());
}

#[test]
fn malformed_metadata_and_source_do_not_claim_a_valid_surface() {
    let workspace = small("module Demo\nexport f\nf(\nend\n");
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-syntax" && d.severity == Severity::Error)
    );
    workspace.write("Project.toml", "name = 42\n");
    assert!(extract(&workspace).metadata.is_none());
}

#[test]
fn imports_reexports_and_assignments_share_canonical_families() {
    let result = extract(&small(
        r#"module Demo
module Inner
export fit
fit(x::Int) = x
end
using .Inner: fit as train
export train, alias
alias = train
end
"#,
    ));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let fit = item(&result, "Demo.Inner.fit");
    assert!(
        fit.aliases
            .iter()
            .any(|alias| alias.qualified_name == "Demo.train")
    );
    assert!(
        fit.aliases
            .iter()
            .any(|alias| alias.qualified_name == "Demo.alias")
    );
    assert_eq!(methods(&result).len(), 1);
}

#[test]
fn docstrings_keep_prose_references_code_and_original_source_ranges() {
    use diplodocus::ir::{Block, Inline};
    let workspace = small(
        r#"module Demo
export fit, score
"""
    Fit α data with **care** and *precision*.

    See [`score`](@ref) and [integer method](@ref score(::Int)).

    - Keep the input.
    - Return a result.

    !!! note "A detail"
        The implementation stays static.

    ```jldoctest
    julia> error("never run this")
    ```
    """
fit(x::Int; options...)::Int = x
@doc "Generic score documentation." function score end
@doc raw"A raw \LaTeX command." score(x::Int) = x
end
"#,
    );
    let result = extract(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let method = result
        .items
        .values()
        .find(|item| item.kind == ItemKind::Method && item.qualified_name == "Demo.fit")
        .unwrap();
    let doc = method.documentation.as_ref().unwrap();
    assert!(
        doc.document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List { items, .. } if items.len() == 2))
    );
    assert!(doc.document.blocks.iter().any(|block| matches!(block, Block::CodeBlock { language, source, .. } if language.as_deref() == Some("julia") && source.contains("never run this"))));
    let refs: Vec<_> = doc
        .document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::Paragraph { inlines, .. } = block {
                Some(inlines)
            } else {
                None
            }
        })
        .flatten()
        .filter_map(|inline| {
            if let Inline::Link {
                target, inlines, ..
            } = inline
            {
                Some((target, inlines))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(refs.len(), 2);
    assert!(refs[0].0.contains("sid1:julia:function:Demo.score"));
    assert!(refs[1].0.contains("sid1:julia:method:Demo.score"));
    assert!(matches!(&refs[1].1[0], Inline::Text { value, .. } if value == "integer method"));
    let original = workspace.read("src/Demo.jl");
    assert!(doc.provenance.iter().all(|p| {
        p.span
            .is_some_and(|span| original.get(span.start..span.end).is_some())
    }));
    assert!(item(&result, "Demo.score").documentation.is_some());
    let decoded = result
        .items
        .values()
        .find(|item| item.kind == ItemKind::Method && item.qualified_name == "Demo.score")
        .unwrap()
        .documentation
        .as_ref()
        .unwrap()
        .raw_source
        .as_ref()
        .unwrap();
    assert_eq!(decoded, r"A raw \LaTeX command.");
}

#[test]
fn unsupported_documentation_remains_visible_and_diagnostics_map_escapes() {
    use diplodocus::ir::Block;
    let source =
        "module Demo\nexport f\n\"Unsupported \\u0060\\u0060x\\u0060\\u0060.\"\nf(x)=x\nend\n";
    let result = extract(&small(source));
    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "julia-unsupported-markdown")
        .unwrap();
    let span = warning.span.unwrap();
    assert_eq!(&source[span.start..span.end], r"\u0060\u0060x\u0060\u0060");
    let result = extract(&small("module Demo\n\"Value: $unknown\"\nf(x)=x\nend\n"));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-unsupported-docstring")
    );
    let method = result
        .items
        .values()
        .find(|item| item.kind == ItemKind::Method)
        .unwrap();
    assert!(
        matches!(&method.documentation.as_ref().unwrap().document.blocks[0], Block::Unsupported { raw, .. } if raw.contains("$unknown"))
    );
}

#[test]
fn family_and_method_references_round_trip_through_snapshots() {
    use diplodocus::assembly::assemble_workspace;
    use diplodocus::snapshots::Snapshot;
    use diplodocus::validation::{resolve_item, resolve_workspace};
    let workspace = small("module Demo\nexport f\n\"See [`f`](@ref).\"\nf(x::Int)=x\nend\n");
    let sources = assemble_workspace(workspace.path().join("diplodocus.toml")).unwrap();
    let family = resolve_item(sources.workspace(), Some("demo"), "Demo.f").unwrap();
    let method = resolve_item(sources.workspace(), Some("demo"), "Demo.f(::Int)").unwrap();
    assert_ne!(family, method);
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let database = workspace.path().join("snapshot.sqlite");
    snapshot.publish(&database).unwrap();
    assert_eq!(
        snapshot.canonical_export().unwrap(),
        Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap()
    );
}

#[test]
fn transparent_wrappers_operators_varargs_and_nonstandard_names_are_static() {
    let result = extract(&small(
        r#"module Demo
export f, +, var"a name"
@inline f(x::Int, args::Float64...; kw...)::Int = x
Base.@noinline function +(x::String, y::String)
    x * y
end
var"a name"(x) = x
end
"#,
    ));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(methods(&result).len(), 3);
    let method = result
        .items
        .values()
        .find(|item| item.kind == ItemKind::Method && item.qualified_name == "Demo.f")
        .unwrap();
    assert_eq!(method.signatures[0].sources.len(), 2);
    assert_eq!(item(&result, "Demo.+").signatures.len(), 1);
    assert_eq!(item(&result, "Demo.a name").signatures.len(), 1);
}

#[test]
fn a_shared_include_is_parsed_once_and_keeps_each_module_context() {
    let workspace = small(
        "module Demo\nmodule A\nexport f\ninclude(\"shared.jl\")\nend\nmodule B\nexport f\ninclude(\"shared.jl\")\nend\nend\n",
    );
    workspace.write("src/shared.jl", "f(x::Int)=x\n");
    let result = extract(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.inputs.len(), 3);
    assert_eq!(methods(&result).len(), 2);
    assert_eq!(
        item(&result, "Demo.A.f").source_location,
        item(&result, "Demo.B.f").source_location
    );
}

#[test]
fn local_using_and_const_aliases_resolve_to_maintained_declarations() {
    let result = extract(&small(
        r#"module Demo
module Inner
export fit
fit(x::Int)=x
end
using .Inner
const train = fit
export train
end
"#,
    ));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let fit = item(&result, "Demo.Inner.fit");
    assert!(
        fit.aliases
            .iter()
            .any(|alias| alias.qualified_name == "Demo.fit")
    );
    assert!(
        fit.aliases
            .iter()
            .any(|alias| alias.qualified_name == "Demo.train")
    );
    assert!(
        !result
            .items
            .values()
            .any(|item| item.qualified_name == "Demo.train")
    );
}

#[test]
fn docs_for_unresolved_aliases_and_reexports_have_visible_errors() {
    for body in [
        "\"Alias docs.\"\nalias = unavailable",
        "using External: f\nexport f",
    ] {
        let result = extract(&small(&format!("module Demo\n{body}\nend\n")));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "julia-unresolved-definition"),
            "{:?}",
            result.diagnostics
        );
    }
}

#[test]
fn included_utf8_failures_retain_original_bytes() {
    let workspace = small("module Demo\ninclude(\"invalid.jl\")\nend\n");
    workspace.write("src/invalid.jl", [0xff, 0x00]);
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-source-read")
    );
    assert!(
        result
            .inputs
            .values()
            .any(|input| input.raw_bytes.as_deref() == Some(&[0xff, 0x00]))
    );
}

#[cfg(unix)]
#[test]
fn includes_reject_symlinks_outside_the_package() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.jl"), "f(x)=x\n").unwrap();
    let workspace = small("module Demo\ninclude(\"escape.jl\")\nend\n");
    std::os::unix::fs::symlink(
        outside.path().join("outside.jl"),
        workspace.path().join("src/escape.jl"),
    )
    .unwrap();
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "source-path-outside-boundary")
    );
    assert_eq!(result.inputs.len(), 2);
}

#[test]
fn a_changed_dispatch_type_changes_identity() {
    let integer = extract(&small("module Demo\nexport f\nf(x::Int)=x\nend\n"));
    let real = extract(&small("module Demo\nexport f\nf(x::Real)=x\nend\n"));
    let vararg = extract(&small("module Demo\nexport f\nf(x::Int...)=x\nend\n"));
    assert_ne!(methods(&integer), methods(&real));
    assert_ne!(methods(&integer), methods(&vararg));
}

#[test]
fn bound_variables_do_not_collide_with_free_type_names() {
    let bound = extract(&small(
        "module Demo\nexport f\nf(x::T, y::T) where T = x\nend\n",
    ));
    let free = extract(&small(
        "module Demo\nexport f\nf(x::T, y::T0) where T = x\nend\n",
    ));
    assert_ne!(methods(&bound), methods(&free));
    let first = extract(&small(
        "module Demo\nexport f\nf(x::Other.T, y::T) where T = x\nend\n",
    ));
    let renamed = extract(&small(
        "module Demo\nexport f\nf(x::Other.T, y::S) where S = x\nend\n",
    ));
    assert_eq!(methods(&first), methods(&renamed));
}

#[test]
fn documented_aliases_keep_their_documentation_on_the_canonical_family() {
    let result = extract(&small(
        "module Demo\nf(x)=x\n\"Alias documentation.\"\nconst alias = f\nend\n",
    ));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        item(&result, "Demo.f")
            .documentation
            .as_ref()
            .unwrap()
            .raw_source
            .as_deref(),
        Some("Alias documentation.")
    );
}

#[test]
fn acceptance_fixture_builds_without_julia_and_retains_structured_markdown() {
    use diplodocus::ir::Block;
    use diplodocus::snapshots::Snapshot;
    use std::path::Path;
    use std::process::Command;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/acceptance/julia");
    let workspace = support::TestWorkspace::new();
    for path in support::files_under(&fixture) {
        workspace.write(&path, std::fs::read(fixture.join(&path)).unwrap());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(workspace.path())
        .args(["build"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!workspace.path().join("julia-was-executed").exists());
    let snapshot =
        Snapshot::load(workspace.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let package = &snapshot.workspace().packages["geometry"];
    let point = package
        .items
        .values()
        .find(|item| item.kind == ItemKind::Type && item.name == "Point")
        .unwrap();
    assert!(
        point
            .documentation
            .as_ref()
            .unwrap()
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { rows, .. } if rows.len() == 3))
    );
    assert!(
        matches!(&point.language_data, Some(ItemLanguageData::Julia(data)) if matches!(&data.declaration, JuliaDeclaration::Type { parameters, constructors, .. } if parameters.len() == 1 && constructors.len() == 1))
    );
    let search: serde_json::Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("site/assets/search.json")).unwrap(),
    )
    .unwrap();
    let generic_method = search
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["title"]
                .as_str()
                .is_some_and(|title| title.contains("values::AbstractVector"))
        })
        .unwrap();
    assert!(
        generic_method["title"]
            .as_str()
            .unwrap()
            .starts_with("TinyGeometry.norm(")
    );
    let second = support::TestWorkspace::new();
    for path in support::files_under(&fixture) {
        second.write(&path, std::fs::read(fixture.join(&path)).unwrap());
    }
    let sources =
        diplodocus::assembly::assemble_workspace(second.path().join("diplodocus.toml")).unwrap();
    let resolved = diplodocus::validation::resolve_workspace(&sources).unwrap();
    let relocated = Snapshot::from_sources(&sources, &resolved).unwrap();
    assert_eq!(
        snapshot.canonical_export().unwrap(),
        relocated.canonical_export().unwrap()
    );
}

#[test]
fn refresh_removes_stale_methods_and_includes_and_preserves_snapshot_on_error() {
    use diplodocus::commands::{self, ExtractOptions};
    use diplodocus::snapshots::Snapshot;
    let workspace = small("module Demo\nexport f\ninclude(\"methods.jl\")\nend\n");
    workspace.write("src/methods.jl", "f(x::Int)=x\nf(x::String)=x\n");
    let database = workspace.path().join("documentation.sqlite");
    let options = || ExtractOptions {
        config: workspace.path().join("diplodocus.toml"),
        output: Some(database.clone()),
    };
    commands::extract(options()).unwrap();
    let before = Snapshot::load(&database).unwrap();
    assert_eq!(
        before.workspace().packages["demo"]
            .items
            .values()
            .filter(|item| item.kind == ItemKind::Method)
            .count(),
        2
    );
    workspace.write(
        "src/Demo.jl",
        "module Demo\nexport f\nf(x::Float64)=x\nend\n",
    );
    commands::extract(options()).unwrap();
    let refreshed = Snapshot::load(&database).unwrap();
    assert_eq!(
        refreshed.workspace().packages["demo"]
            .items
            .values()
            .filter(|item| item.kind == ItemKind::Method)
            .count(),
        1
    );
    assert!(
        !refreshed
            .canonical_export()
            .unwrap()
            .contains("src/methods.jl")
    );
    let successful = std::fs::read(&database).unwrap();
    workspace.write(
        "src/Demo.jl",
        "module Demo\nexport f\ninclude(\"missing.jl\")\nend\n",
    );
    assert!(commands::extract(options()).is_err());
    assert_eq!(std::fs::read(&database).unwrap(), successful);
}

#[test]
fn unresolved_doc_references_point_to_original_literal_bytes() {
    let source = "module Demo\r\nexport f\r\n\"\"\"\r\n    α documentation.\r\n    [missing](@ref unavailable)\r\n    \"\"\"\r\nf(x)=x\r\nend\r\n";
    let workspace = small(source);
    let sources =
        diplodocus::assembly::assemble_workspace(workspace.path().join("diplodocus.toml")).unwrap();
    let diplodocus::validation::ResolutionError::Diagnostics(diagnostics) =
        diplodocus::validation::resolve_workspace(&sources).unwrap_err()
    else {
        panic!("expected an unresolved reference")
    };
    let diagnostic = diagnostics
        .iter()
        .find(|d| d.code.as_str() == "unresolved-item-reference")
        .unwrap();
    let span = diagnostic.span.unwrap();
    assert_eq!(&source[span.start..span.end], "[missing](@ref unavailable)");
}

#[test]
fn project_metadata_preserves_julia_compatibility_and_optional_version() {
    let workspace = small("module Demo\nend\n");
    let text = "name = \"Demo\"\nuuid = \"12345678-1234-1234-1234-123456789abc\"\nauthors = [\"A Researcher\"]\n[deps]\nOther = \"87654321-4321-4321-4321-cba987654321\"\n[compat]\njulia = \"1.6 - 1.12\"\nOther = \"0.3, ^1\"\n";
    workspace.write("Project.toml", text);
    let result = extract(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let metadata = result.metadata.unwrap();
    assert!(metadata.version.is_none());
    assert_eq!(metadata.compatibility["julia"], "1.6 - 1.12");
    assert_eq!(
        metadata.dependencies["Other"],
        "87654321-4321-4321-4321-cba987654321"
    );
    let span = metadata.fields["name"].source.span.unwrap();
    assert_eq!(&text[span.start..span.end], "\"Demo\"");
    let sources =
        diplodocus::assembly::assemble_workspace(workspace.path().join("diplodocus.toml")).unwrap();
    assert!(sources.workspace().packages["demo"].version.is_none());
}

#[test]
fn a_shadowed_include_cannot_claim_standard_include_semantics() {
    let workspace =
        small("module Demo\nexport f\ninclude(path) = nothing\ninclude(\"methods.jl\")\nend\n");
    workspace.write("src/methods.jl", "f(x)=x\n");
    let result = extract(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-include")
    );
}

#[test]
fn anonymous_vararg_selectors_match_named_vararg_methods() {
    let named = extract(&small(
        "module Demo\nexport f\nf(values::Int...) = values\nend\n",
    ));
    let anonymous = extract(&small(
        "module Demo\nexport f\nf(::Int...) = nothing\nend\n",
    ));
    assert!(
        anonymous.diagnostics.is_empty(),
        "{:?}",
        anonymous.diagnostics
    );
    assert_eq!(methods(&named), methods(&anonymous));
    let workspace = small(
        "module Demo\nexport f\n\"See [varargs](@ref f(::Int...)).\"\nfunction f end\nf(values::Int...) = values\nend\n",
    );
    let sources =
        diplodocus::assembly::assemble_workspace(workspace.path().join("diplodocus.toml")).unwrap();
    diplodocus::validation::resolve_workspace(&sources).unwrap();
}

#[test]
fn incompatible_declarations_at_one_binding_are_diagnosed() {
    let result = extract(&small(
        "module Demo\nexport f\nconst f = 42\nf(x) = x\nend\n",
    ));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "julia-conflicting-surface")
    );
}

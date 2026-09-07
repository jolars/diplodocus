const CONTRACT: &str = include_str!("../docs/spikes/static-extractor-contract.md");
const MANIFEST: &str = include_str!("../Cargo.toml");

#[test]
fn parser_versions_are_pinned_and_match_the_contract() {
    for (dependency, manifest_declaration, contract_declaration) in [
        (
            "pyproject-toml",
            "pyproject-toml = \"=0.13.7\"",
            "| `pyproject-toml` | `0.13.7` |",
        ),
        (
            "ruff_python_parser",
            "ruff_python_parser = \"=0.0.12\"",
            "| `ruff_python_parser` | `0.0.12` |",
        ),
        (
            "ruff_python_ast",
            "ruff_python_ast = \"=0.0.12\"",
            "| `ruff_python_ast` | `0.0.12` |",
        ),
        (
            "ruff_text_size",
            "ruff_text_size = \"=0.0.12\"",
            "| `ruff_text_size` | `0.0.12` |",
        ),
        (
            "pydocstring",
            "pydocstring = \"=0.4.1\"",
            "| `pydocstring` | `0.4.1` |",
        ),
        (
            "arity-parser",
            "arity-parser = \"=0.6.0\"",
            "| `arity-parser` | `0.6.0` |",
        ),
        (
            "rd-source",
            "rd-source = \"=0.4.0\"",
            "| `rd-source` | `0.4.0` |",
        ),
        (
            "rd-ast",
            "rd-ast = { version = \"=0.4.0\", default-features = false }",
            "| `rd-ast` | `0.4.0` |",
        ),
    ] {
        assert!(
            MANIFEST.contains(manifest_declaration),
            "{dependency} must remain exactly pinned in Cargo.toml"
        );
        assert!(
            CONTRACT.contains(contract_declaration),
            "{dependency} must have the same version in the extractor contract"
        );
    }
}

#[test]
fn both_extractors_have_complete_capability_manifests() {
    assert_eq!(
        capabilities_between("## Python extractor", "## R extractor"),
        [
            "diagnostics.unsupported-visible",
            "provenance.source",
            "python.declarations",
            "python.docs.numpy",
            "python.exports.static",
            "python.metadata.pep621",
            "python.overloads",
            "python.reexports",
            "python.stubs",
        ]
    );
    assert_eq!(
        capabilities_between("## R extractor", "## Provenance fields"),
        [
            "diagnostics.unsupported-visible",
            "provenance.source",
            "r.docs.rd",
            "r.metadata.dcf",
            "r.namespace.static",
            "r.s3",
            "r.source.functions",
        ]
    );
}

#[test]
fn provenance_contract_defines_every_required_scope() {
    for field in [
        "`extractor`",
        "`mode`",
        "`capabilities`",
        "`parsers`",
        "`target`",
        "`inputs`",
        "`repository_id`",
        "`package_id`",
        "`target_id`",
        "`path`",
        "`role`",
        "`fingerprint`",
        "`range`",
        "`parser`",
    ] {
        assert!(
            CONTRACT.contains(field),
            "extractor contract is missing provenance field {field}"
        );
    }
}

fn capabilities_between(section_start: &str, section_end: &str) -> Vec<&'static str> {
    let section = CONTRACT
        .split_once(section_start)
        .unwrap_or_else(|| panic!("missing contract section {section_start}"))
        .1
        .split_once(section_end)
        .unwrap_or_else(|| panic!("missing contract section {section_end}"))
        .0;
    let table = section
        .split_once("### Capability manifest")
        .unwrap_or_else(|| panic!("missing capability manifest in {section_start}"))
        .1;

    table
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split_once('`').map(|(capability, _)| capability))
        .collect()
}

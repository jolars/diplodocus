use super::{CanonicalError, CanonicalValue};
use CanonicalValue as V;
use std::collections::{BTreeMap, BTreeSet};

/// Check the exact closed key schema before a cache consumer compares identities.
/// Decoding data never constructs a trusted `ExecutionIdentity`.
pub fn validate_key_input(value: &CanonicalValue) -> Result<(), CanonicalError> {
    let key = value.fields(&[
        "schema",
        "schemas",
        "page",
        "options",
        "engine",
        "policies",
        "components",
        "kernel",
        "platform",
        "deadlines_ms",
        "environment_inputs",
    ])?;
    literal(&key["schema"], "page-execution-key-v1")?;
    let schemas = key["schemas"].fields(&["encoding", "artifact", "ir"])?;
    for (field, expected) in [
        ("encoding", "execution-json-v1"),
        ("artifact", "page-execution-artifact-v1"),
        ("ir", "execution-result-v1"),
    ] {
        literal(&schemas[field], expected)?;
    }
    let page = key["page"].fields(&[
        "repository",
        "collection",
        "path",
        "working_directory",
        "format",
        "source_digest",
    ])?;
    nonempty(&page["repository"])?;
    nonempty(&page["collection"])?;
    path(&page["path"], false)?;
    path(&page["working_directory"], true)?;
    literal(&page["format"], "qmd")?;
    digest(&page["source_digest"])?;
    let options = key["options"].fields(&["mode", "page_veto", "defaults", "cells"])?;
    literal(&options["mode"], "execute")?;
    if options["page_veto"] != V::Bool(false) {
        return Err(CanonicalError);
    }
    validate_options(&options["defaults"], false)?;
    let cells = array(&options["cells"])?;
    for (ordinal, cell) in cells.iter().enumerate() {
        let cell = cell.fields(&[
            "ordinal",
            "language",
            "eligible",
            "submitted_source_digest",
            "effective",
        ])?;
        if number(&cell["ordinal"])? != ordinal as u64 {
            return Err(CanonicalError);
        }
        nullable_string(&cell["language"])?;
        if let V::String(language) = &cell["language"]
            && (language.is_empty() || super::local::normalize_language(language) != *language)
        {
            return Err(CanonicalError);
        }
        let eligible = boolean(&cell["eligible"])?;
        if eligible {
            digest(&cell["submitted_source_digest"])?;
        } else if cell["submitted_source_digest"] != V::Null {
            return Err(CanonicalError);
        }
        validate_options(&cell["effective"], true)?;
    }
    let engine = key["engine"].fields(&["id", "version", "build_digest"])?;
    literal(&engine["id"], "jupyter")?;
    version(&engine["version"])?;
    digest(&engine["build_digest"])?;
    let policies = key["policies"].fields(&["qmd", "execution", "mime", "html", "svg"])?;
    for value in policies.values() {
        nonempty(value)?;
    }
    let mut last = None;
    let mut roles = BTreeSet::new();
    for component in array(&key["components"])? {
        let component = component.fields(&["role", "name", "version"])?;
        let tuple = (
            nonempty(&component["role"])?,
            nonempty(&component["name"])?,
            version(&component["version"])?,
        );
        if last.is_some_and(|previous| previous >= tuple) || !roles.insert(tuple.0) {
            return Err(CanonicalError);
        }
        last = Some(tuple);
    }
    for role in [
        "async-runtime",
        "authored-parser",
        "fragment-parser",
        "html-parser",
        "html-sanitizer",
        "jupyter-protocol",
        "jupyter-transport",
        "raster-decoder",
        "raster-validator",
        "svg-parser",
        "svg-validator",
    ] {
        if !roles.contains(role) {
            return Err(CanonicalError);
        }
    }
    let kernel = key["kernel"].fields(&[
        "name",
        "spec_digest",
        "launch_digest",
        "search",
        "interrupt_mode",
        "runtime",
    ])?;
    let name = nonempty(&kernel["name"])?;
    if name != name.to_ascii_lowercase()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    {
        return Err(CanonicalError);
    }
    digest(&kernel["spec_digest"])?;
    digest(&kernel["launch_digest"])?;
    one_of(&kernel["interrupt_mode"], &["signal", "message"])?;
    let mut ordinals = BTreeMap::new();
    let mut selected = 0;
    for search in array(&kernel["search"])? {
        let search = search.fields(&["class", "ordinal", "selected"])?;
        let class = one_of(
            &search["class"],
            &["jupyter-path", "user-data", "system-local", "system"],
        )?;
        let expected = ordinals.entry(class).or_insert(0);
        if number(&search["ordinal"])? != *expected {
            return Err(CanonicalError);
        }
        *expected += 1;
        selected += usize::from(boolean(&search["selected"])?);
    }
    if selected != 1 {
        return Err(CanonicalError);
    }
    let runtime = kernel["runtime"].fields(&[
        "implementation",
        "implementation_version",
        "language",
        "language_version",
        "protocol_version",
    ])?;
    for value in runtime.values() {
        nonempty(value)?;
    }
    let language = string(&runtime["language"])?;
    if super::local::normalize_language(language) != language {
        return Err(CanonicalError);
    }
    let mut any_eligible = false;
    for cell in cells {
        let V::Object(cell) = cell else {
            return Err(CanonicalError);
        };
        let V::Object(effective) = &cell["effective"] else {
            return Err(CanonicalError);
        };
        let eligible =
            cell["language"] == V::String(language.into()) && effective["eval"] == V::Bool(true);
        if cell["eligible"] != V::Bool(eligible) {
            return Err(CanonicalError);
        }
        any_eligible |= eligible;
    }
    if !any_eligible {
        return Err(CanonicalError);
    }
    let platform = key["platform"].fields(&["os", "architecture", "target"])?;
    literal(&platform["os"], "linux")?;
    nonempty(&platform["architecture"])?;
    nonempty(&platform["target"])?;
    let deadlines = key["deadlines_ms"].fields(&[
        "startup",
        "cell",
        "terminal_sync",
        "interrupt",
        "shutdown",
        "termination",
        "forced_exit",
    ])?;
    for deadline in deadlines.values() {
        if number(deadline)? == 0 {
            return Err(CanonicalError);
        }
    }
    let mut last = None;
    for input in array(&key["environment_inputs"])? {
        let input = input.fields(&["repository", "path", "digest"])?;
        let entry = (
            nonempty(&input["repository"])?,
            path(&input["path"], false)?,
        );
        if last.is_some_and(|previous| previous >= entry) {
            return Err(CanonicalError);
        }
        digest(&input["digest"])?;
        last = Some(entry);
    }
    Ok(())
}
fn validate_options(value: &V, cell: bool) -> Result<(), CanonicalError> {
    let fields = if cell {
        value.fields(&[
            "eval",
            "echo",
            "output",
            "include",
            "error",
            "label",
            "fig-alt",
            "fig-cap",
            "fig-subcap",
        ])?
    } else {
        value.fields(&["eval", "echo", "output", "include", "error"])?
    };
    for name in ["eval", "echo", "include", "error"] {
        boolean(&fields[name])?;
    }
    if !matches!(fields["output"], V::Bool(_)) {
        literal(&fields["output"], "asis")?;
    }
    if cell {
        for name in ["label", "fig-alt", "fig-cap"] {
            nullable_string(&fields[name])?;
        }
        for value in array(&fields["fig-subcap"])? {
            string(value)?;
        }
    }
    Ok(())
}
fn string(value: &V) -> Result<&str, CanonicalError> {
    if let V::String(s) = value {
        Ok(s)
    } else {
        Err(CanonicalError)
    }
}
fn nonempty(value: &V) -> Result<&str, CanonicalError> {
    let s = string(value)?;
    if s.is_empty() || s.contains('\0') {
        Err(CanonicalError)
    } else {
        Ok(s)
    }
}
fn nullable_string(value: &V) -> Result<(), CanonicalError> {
    if *value == V::Null {
        Ok(())
    } else {
        string(value).map(|_| ())
    }
}
fn literal(value: &V, expected: &str) -> Result<(), CanonicalError> {
    if string(value)? == expected {
        Ok(())
    } else {
        Err(CanonicalError)
    }
}
fn one_of<'a>(value: &'a V, choices: &[&str]) -> Result<&'a str, CanonicalError> {
    let s = string(value)?;
    if choices.contains(&s) {
        Ok(s)
    } else {
        Err(CanonicalError)
    }
}
fn number(value: &V) -> Result<u64, CanonicalError> {
    if let V::Integer(v) = value {
        Ok(*v)
    } else {
        Err(CanonicalError)
    }
}
fn boolean(value: &V) -> Result<bool, CanonicalError> {
    if let V::Bool(v) = value {
        Ok(*v)
    } else {
        Err(CanonicalError)
    }
}
fn array(value: &V) -> Result<&[V], CanonicalError> {
    if let V::Array(v) = value {
        Ok(v)
    } else {
        Err(CanonicalError)
    }
}
fn digest(value: &V) -> Result<(), CanonicalError> {
    super::local::fingerprint(string(value)?)
        .map(|_| ())
        .map_err(|_| CanonicalError)
}
fn path(value: &V, directory: bool) -> Result<&str, CanonicalError> {
    let s = string(value)?;
    if directory && s == "." {
        return Ok(s);
    }
    crate::diagnostics::DiagnosticPath::try_from(s).map_err(|_| CanonicalError)?;
    Ok(s)
}

fn version(value: &V) -> Result<&str, CanonicalError> {
    let value = nonempty(value)?;
    if !super::local::exact_version(value) {
        return Err(CanonicalError);
    }
    Ok(value)
}

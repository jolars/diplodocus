//! Shared by the build script and focused graph-selection tests.

pub fn selected_version(
    lock: &str,
    root_version: &str,
    name: &str,
) -> Result<String, &'static str> {
    let document: toml::Value = toml::from_str(lock).map_err(|_| "invalid lockfile")?;
    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("missing packages")?;
    let roots: Vec<_> = packages
        .iter()
        .filter(|p| {
            p.get("name").and_then(toml::Value::as_str) == Some("diplodocus")
                && p.get("version").and_then(toml::Value::as_str) == Some(root_version)
        })
        .collect();
    let [root] = roots.as_slice() else {
        return Err("missing or ambiguous root");
    };
    let dependencies = root
        .get("dependencies")
        .and_then(toml::Value::as_array)
        .ok_or("missing dependencies")?;
    let edges: Vec<_> = dependencies
        .iter()
        .filter_map(toml::Value::as_str)
        .filter(|edge| edge.split_whitespace().next() == Some(name))
        .collect();
    let [edge] = edges.as_slice() else {
        return Err("missing or ambiguous dependency edge");
    };
    let mut parts = edge.split_whitespace();
    parts.next();
    let version = parts.next();
    let source = parts.next().map(|s| s.trim_matches(['(', ')']));
    if parts.next().is_some() {
        return Err("invalid dependency edge");
    }
    let candidates: Vec<_> = packages
        .iter()
        .filter(|p| {
            p.get("name").and_then(toml::Value::as_str) == Some(name)
                && version.is_none_or(|v| p.get("version").and_then(toml::Value::as_str) == Some(v))
                && source.is_none_or(|s| p.get("source").and_then(toml::Value::as_str) == Some(s))
        })
        .collect();
    let [package] = candidates.as_slice() else {
        return Err("missing or ambiguous selected package");
    };
    package
        .get("version")
        .and_then(toml::Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or("missing selected version")
}

pub fn components(
    lock: &str,
    version: &str,
) -> Result<Vec<(String, String, String)>, &'static str> {
    let mut components = Vec::new();
    for (role, name) in [
        ("async-runtime", "tokio"),
        ("authored-parser", "panache-parser"),
        ("fragment-parser", "panache-parser"),
        ("html-parser", "html5ever"),
        ("html-dom", "markup5ever_rcdom"),
        ("jupyter-protocol", "jupyter-protocol"),
        ("jupyter-transport", "jupyter-zmq-client"),
        ("raster-decoder", "image"),
        ("png-decoder", "png"),
        ("jpeg-decoder", "zune-jpeg"),
        ("jpeg-core", "zune-core"),
        ("svg-parser", "roxmltree"),
        ("svg-value-parser", "svgtypes"),
        ("url-parser", "url"),
        ("percent-decoder", "percent-encoding"),
    ] {
        components.push((
            role.into(),
            name.into(),
            selected_version(lock, version, name)?,
        ));
    }
    for (role, name) in [
        ("html-sanitizer", "diplodocus-html-sanitizer"),
        ("raster-validator", "diplodocus-raster-validator"),
        ("svg-validator", "diplodocus-svg-validator"),
    ] {
        components.push((role.into(), name.into(), version.into()));
    }
    components.sort();
    Ok(components)
}

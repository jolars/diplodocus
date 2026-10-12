use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use rd_rds::lazyload::LazyLoadIndex;
use rd_rds::package::{MetadataField, NamespaceMetadata};

use crate::ir::{ParserProvenance, RGenericReference};

// Profiles and default package attachment are disabled so discovery evaluates
// only base metadata and cannot run a project's startup or package hooks.
const PROBE: &str = r#"
writeLines(paste("library", .libPaths(), sep = "\t"))
writeLines(paste("base", ls(baseenv(), all.names = TRUE), sep = "\t"))
writeLines(paste("generic", names(.knownS3Generics), .knownS3Generics, sep = "\t"))
writeLines(paste("version", getRversion(), sep = "\t"))
"#;

#[derive(Clone)]
struct Discovery {
    paths: Vec<PathBuf>,
    base: BTreeSet<String>,
    generics: BTreeMap<String, String>,
    version: String,
}

pub(super) struct Libraries {
    discovery: Discovery,
    exports: BTreeMap<String, BTreeSet<String>>,
}

impl Libraries {
    pub(super) fn discover() -> Result<Self, String> {
        static DISCOVERY: OnceLock<Result<Discovery, String>> = OnceLock::new();
        let discovery = DISCOVERY.get_or_init(probe).clone()?;
        let mut libraries = Self {
            exports: BTreeMap::from([("base".into(), discovery.base.clone())]),
            discovery,
        };
        for package in [
            "stats",
            "graphics",
            "grDevices",
            "utils",
            "datasets",
            "methods",
        ] {
            libraries.exports(package)?;
        }
        Ok(libraries)
    }

    pub(super) fn exports(&mut self, package: &str) -> Result<&BTreeSet<String>, String> {
        if !self.exports.contains_key(package) {
            if !valid_package_name(package) {
                return Err(format!("Invalid installed R package name `{package}`."));
            }
            let directory = self
                .discovery
                .paths
                .iter()
                .map(|library| library.join(package))
                .find(|directory| directory.join("DESCRIPTION").is_file())
                .ok_or_else(|| {
                    format!("R package `{package}` is not installed in the discovered libraries.")
                })?;
            let exports = read_exports(&directory).map_err(|_| {
                format!(
                    "Cannot read complete installed namespace metadata for R package `{package}`."
                )
            })?;
            self.exports.insert(package.into(), exports);
        }
        Ok(&self.exports[package])
    }

    pub(super) fn default_generic(&self, name: &str) -> Option<RGenericReference> {
        // A registration establishes the generic role. Installed exports and
        // R's own known-generic map establish where that name is available.
        let package = self
            .discovery
            .generics
            .get(name)
            .map(String::as_str)
            .or_else(|| {
                [
                    "base",
                    "stats",
                    "graphics",
                    "grDevices",
                    "utils",
                    "datasets",
                    "methods",
                ]
                .into_iter()
                .find(|package| self.exports[*package].contains(name))
            })?;
        Some(RGenericReference::External {
            package: package.into(),
            name: name.into(),
        })
    }

    pub(super) fn provenance(&self) -> ParserProvenance {
        let mut settings = BTreeMap::from([("r-version".into(), self.discovery.version.clone())]);
        for (package, exports) in &self.exports {
            let bytes = serde_json::to_vec(exports).expect("string sets serialize");
            settings.insert(
                format!("exports:{package}"),
                crate::provenance::fingerprint_bytes(&bytes).value,
            );
        }
        let bytes = serde_json::to_vec(&self.discovery.generics).expect("string maps serialize");
        settings.insert(
            "base-known-s3-generics".into(),
            crate::provenance::fingerprint_bytes(&bytes).value,
        );
        ParserProvenance {
            version: "0.5.0".into(),
            role: "r-installed-namespace-metadata".into(),
            settings,
        }
    }
}

fn probe() -> Result<Discovery, String> {
    let output = Command::new("R")
        .args(["--vanilla", "--slave", "-e", PROBE])
        .env("R_DEFAULT_PACKAGES", "NULL")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            "R extraction requires an installed R executable on PATH to discover package libraries."
                .to_owned()
        })?;
    if !output.status.success() {
        return Err("R failed to report its installed package libraries and base metadata.".into());
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "R library discovery returned invalid UTF-8.".to_owned())?;
    let mut discovery = Discovery {
        paths: vec![],
        base: BTreeSet::new(),
        generics: BTreeMap::new(),
        version: String::new(),
    };
    for line in text.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        match fields.as_slice() {
            ["library", path] if !path.is_empty() => discovery.paths.push(PathBuf::from(path)),
            ["base", name] if !name.is_empty() => {
                discovery.base.insert((*name).into());
            }
            ["generic", name, package] if !name.is_empty() && valid_package_name(package) => {
                discovery.generics.insert((*name).into(), (*package).into());
            }
            ["version", version] if !version.is_empty() => {
                discovery.version = (*version).into();
            }
            _ => return Err("R library discovery returned malformed metadata.".into()),
        }
    }
    if discovery.paths.is_empty()
        || discovery.base.is_empty()
        || discovery.generics.is_empty()
        || discovery.version.is_empty()
    {
        return Err("R library discovery returned incomplete metadata.".into());
    }
    Ok(discovery)
}

fn valid_package_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
}

fn read_exports(directory: &std::path::Path) -> Result<BTreeSet<String>, ()> {
    let namespace = NamespaceMetadata::read_installed(directory).map_err(|_| ())?;
    let MetadataField::Present(declared) = namespace.declared_exports() else {
        return Err(());
    };
    let MetadataField::Present(patterns) = namespace.export_patterns() else {
        return Err(());
    };
    let mut exports: BTreeSet<_> = declared
        .iter()
        .map(|export| export.exported_name().to_owned())
        .collect();
    if !patterns.is_empty() {
        let package = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(())?;
        let index = LazyLoadIndex::open(directory.join("R").join(format!("{package}.rdx")))
            .map_err(|_| ())?;
        for pattern in patterns {
            let pattern = regex::Regex::new(pattern).map_err(|_| ())?;
            exports.extend(
                index
                    .variables()
                    .iter()
                    .filter(|binding| pattern.is_match(binding.name()))
                    .map(|binding| binding.name().to_owned()),
            );
        }
    }
    Ok(exports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r-installed-library")
    }

    #[test]
    fn installed_exports_need_neither_code_records_nor_package_loading() {
        let root = fixture();
        assert_eq!(
            read_exports(&root.join("direct")).unwrap(),
            BTreeSet::from(["foreignGeneric".into()])
        );
        assert_eq!(
            read_exports(&root.join("patterns")).unwrap(),
            BTreeSet::from(["foreignGeneric".into(), "publicGeneric".into()])
        );
        assert!(read_exports(&root.join("malformed")).is_err());
        assert!(read_exports(&root.join("missing")).is_err());
    }

    #[test]
    fn library_precedence_and_failed_metadata_do_not_fall_back_to_other_installs() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("direct")).unwrap();
        std::fs::write(temp.path().join("direct/DESCRIPTION"), "Package: direct\n").unwrap();
        let discovery = Discovery {
            paths: vec![temp.path().into(), fixture()],
            base: BTreeSet::new(),
            generics: BTreeMap::new(),
            version: "test".into(),
        };
        let mut libraries = Libraries {
            discovery,
            exports: BTreeMap::new(),
        };
        assert!(libraries.exports("direct").is_err());
        assert!(libraries.exports("../direct").is_err());
        assert!(libraries.exports("notInstalled").is_err());
        assert!(
            libraries
                .exports("patterns")
                .unwrap()
                .contains("publicGeneric")
        );
    }
}

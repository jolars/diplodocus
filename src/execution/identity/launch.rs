use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::local::{hash, normalize_language, read_regular, roots};
use super::{IdentityError, content_digest};
use crate::execution::{KernelInterruptMode, KernelSearchLocation};

/// Captured resolver facts. This local value has redacted Debug and no Serde.
#[derive(Clone)]
pub struct LaunchResolverInput {
    /// All configured canonical roots, including aliases and nested roots.
    pub repositories: BTreeMap<String, PathBuf>,
    /// The selected kernelspec file; identity rereads its behavioral fields.
    pub spec_path: PathBuf,
    /// Explicit configured selector.
    pub selector: String,
    /// Portable ordered observations from discovery.
    pub search: Vec<KernelSearchLocation>,
    /// Captured PATH used only when `argv[0]` is a bare command.
    pub executable_search_path: OsString,
    /// Owning repository identifier for the actual working directory.
    pub repository: String,
    /// Normalized relative directory, with `.` for the repository root.
    pub working_directory: String,
}

impl fmt::Debug for LaunchResolverInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LaunchResolverInput { <private> }")
    }
}

/// One resolved launch observation. Spawn adapters use these same immutable facts.
///
/// ```compile_fail
/// use diplodocus::execution::identity::LaunchIdentityInput;
/// fn serialize<T: serde::Serialize>() {}
/// serialize::<LaunchIdentityInput>();
/// ```
#[derive(Clone)]
pub struct LaunchIdentityInput {
    resolver: LaunchResolverInput,
    executable: PathBuf,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    cwd: PathBuf,
    spec_digest: String,
    launch_digest: String,
    language: String,
    interrupt_mode: KernelInterruptMode,
}

impl fmt::Debug for LaunchIdentityInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LaunchIdentityInput { <private> }")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    argv: Vec<String>,
    language: String,
    #[serde(default)]
    interrupt_mode: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default, rename = "display_name")]
    _display_name: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
}

impl LaunchIdentityInput {
    /// Resolve once without launching a process or probing the kernel.
    pub async fn resolve(resolver: LaunchResolverInput) -> Result<Self, IdentityError> {
        roots(&resolver.repositories).await?;
        if resolver.selector.is_empty()
            || !resolver
                .selector
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
        {
            return Err(IdentityError);
        }
        validate_search(&resolver.search)?;
        let root = resolver
            .repositories
            .get(&resolver.repository)
            .ok_or(IdentityError)?;
        let relative = &resolver.working_directory;
        if relative != "." {
            crate::diagnostics::DiagnosticPath::try_from(relative.as_str())
                .map_err(|_| IdentityError)?;
        }
        let cwd = root.join(relative);
        let resolved_cwd = tokio::fs::canonicalize(&cwd)
            .await
            .map_err(|_| IdentityError)?;
        if !resolved_cwd.starts_with(root)
            || !tokio::fs::metadata(&resolved_cwd)
                .await
                .map_err(|_| IdentityError)?
                .is_dir()
        {
            return Err(IdentityError);
        }
        let (_, bytes) = read_regular(&resolver.spec_path).await?;
        // Restricted parsing rejects duplicates before Serde can discard them.
        // Kernelspec metadata is presentation data; numeric metadata is allowed.
        let checked: DuplicateChecked =
            serde_json::from_slice(&bytes).map_err(|_| IdentityError)?;
        let spec: Spec = serde_json::from_value(checked.0).map_err(|_| IdentityError)?;
        if spec
            .argv
            .first()
            .is_none_or(|s| s.is_empty() || s.contains("{connection_file}"))
            || !spec
                .argv
                .iter()
                .skip(1)
                .any(|s| s.contains("{connection_file}"))
            || spec.argv.iter().any(|s| s.contains('\0'))
            || spec.language.trim().is_empty()
            || spec.metadata.contains_key("kernel_provisioner")
            || spec.metadata.contains_key("supported_encryption")
            || spec.env.iter().any(|(k, v)| {
                k.is_empty() || k.contains(['=', '\0']) || v.contains('\0') || v.contains("${")
            })
        {
            return Err(IdentityError);
        }
        let interrupt_mode = match spec.interrupt_mode.as_deref() {
            None | Some("signal") => KernelInterruptMode::Signal,
            Some("message") => KernelInterruptMode::Message,
            _ => return Err(IdentityError),
        };
        let language = normalize_language(&spec.language);
        let argv: Vec<_> = spec
            .argv
            .iter()
            .enumerate()
            .map(|(i, s)| segments(s, i != 0, &resolver.repositories))
            .collect::<Result<_, _>>()?;
        let env: Vec<_> = spec
            .env
            .iter()
            .map(|(name, value)| {
                Ok(json!({"name":name,"value":segments(value, false, &resolver.repositories)?}))
            })
            .collect::<Result<_, IdentityError>>()?;
        let spec_digest = hash(
            "diplodocus/execution-kernelspec-v1",
            json!({"argv":argv,"language":language,"interrupt_mode":interrupt_mode,"env":env}),
        )?;
        let executable =
            resolve_executable(&spec.argv[0], &resolver.executable_search_path, &cwd).await?;
        let (_, executable_bytes) = read_regular(&executable).await?;
        let location = repository_location(&executable, &resolver.repositories).unwrap_or_else(|| json!({"kind":"external","path_digest":content_digest(executable.to_str().unwrap().as_bytes())}));
        let mut arguments = spec.argv;
        arguments[0] = executable.to_str().ok_or(IdentityError)?.into();
        let argv: Vec<_> = arguments
            .iter()
            .enumerate()
            .map(|(i, s)| segments(s, i != 0, &resolver.repositories))
            .collect::<Result<_, _>>()?;
        let launch_digest = hash(
            "diplodocus/execution-launch-v1",
            json!({"spec_digest":spec_digest,"executable":{"location":location,"digest":content_digest(&executable_bytes)},"argv":argv,"env":env,"working_directory":{"repository":resolver.repository,"path":relative}}),
        )?;
        Ok(Self {
            resolver,
            executable,
            arguments,
            environment: spec.env,
            cwd: resolved_cwd,
            spec_digest,
            launch_digest,
            language,
            interrupt_mode,
        })
    }

    /// Canonical resolved executable; a process adapter must not resolve PATH again.
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    /// Actual cwd used by both hashing and the process adapter.
    pub fn working_directory(&self) -> &Path {
        &self.cwd
    }
    /// Explicit overrides only; inherited values are intentionally absent.
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    /// Materialize only the validated argv substitution positions.
    pub fn arguments(&self, connection_file: &Path) -> Result<Vec<String>, IdentityError> {
        let connection = connection_file
            .to_str()
            .filter(|s| !s.contains('\0'))
            .ok_or(IdentityError)?;
        Ok(self
            .arguments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i == 0 {
                    s.clone()
                } else {
                    s.replace("{connection_file}", connection)
                }
            })
            .collect())
    }
    /// Portable private-record fingerprint.
    pub fn spec_digest(&self) -> &str {
        &self.spec_digest
    }
    /// Portable private-record fingerprint.
    pub fn launch_digest(&self) -> &str {
        &self.launch_digest
    }
    /// Normalized discovered language.
    pub fn language(&self) -> &str {
        &self.language
    }
    /// Validated interrupt behavior.
    pub fn interrupt_mode(&self) -> KernelInterruptMode {
        self.interrupt_mode
    }
    /// Normalized configured kernel selector.
    pub fn selector(&self) -> String {
        self.resolver.selector.to_ascii_lowercase()
    }
    /// Ordered portable discovery observations.
    pub fn search(&self) -> &[KernelSearchLocation] {
        &self.resolver.search
    }
    pub(super) fn resolver(&self) -> &LaunchResolverInput {
        &self.resolver
    }
    /// Recheck resolver, selected spec, executable bytes, and cwd before spawn.
    pub async fn revalidate(&self) -> Result<(), IdentityError> {
        let current = Self::resolve(self.resolver.clone()).await?;
        if !self.same_observation(&current) {
            return Err(IdentityError);
        }
        Ok(())
    }
    pub(super) fn same_observation(&self, current: &Self) -> bool {
        self.spec_digest == current.spec_digest
            && self.launch_digest == current.launch_digest
            && self.executable == current.executable
            && self.cwd == current.cwd
            && self.search() == current.search()
            && self.selector() == current.selector()
    }
}

async fn resolve_executable(
    command: &str,
    path: &std::ffi::OsStr,
    cwd: &Path,
) -> Result<PathBuf, IdentityError> {
    let candidates = if command.contains('/') {
        vec![cwd.join(command)]
    } else {
        std::env::split_paths(path)
            .map(|part| cwd.join(part).join(command))
            .collect()
    };
    for candidate in candidates {
        if let Ok(resolved) = tokio::fs::canonicalize(&candidate).await {
            let metadata = tokio::fs::metadata(&resolved)
                .await
                .map_err(|_| IdentityError)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            if metadata.is_file() && resolved.to_str().is_some() {
                return Ok(resolved);
            }
        }
    }
    Err(IdentityError)
}

pub(super) fn validate_search(search: &[KernelSearchLocation]) -> Result<(), IdentityError> {
    let mut counts = BTreeMap::new();
    if search.iter().filter(|s| s.selected).count() != 1 {
        return Err(IdentityError);
    }
    for location in search {
        let class = format!("{:?}", location.class);
        let ordinal = counts.entry(class).or_insert(0);
        if location.ordinal != *ordinal {
            return Err(IdentityError);
        }
        *ordinal += 1;
    }
    Ok(())
}

fn repository_location(path: &Path, roots: &BTreeMap<String, PathBuf>) -> Option<Value> {
    let (id, root) = roots
        .iter()
        .filter(|(_, root)| path.starts_with(root))
        .min_by(|(a, ar), (b, br)| {
            br.as_os_str()
                .len()
                .cmp(&ar.as_os_str().len())
                .then(a.cmp(b))
        })?;
    let relative = path.strip_prefix(root).ok()?.to_str()?;
    Some(
        json!({"kind":"repository","repository":id,"path":if relative.is_empty() { "." } else { relative }}),
    )
}

fn segments(
    value: &str,
    connection: bool,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<Vec<Value>, IdentityError> {
    let mut output = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    while index < value.len() {
        let tail = &value[index..];
        if connection && tail.starts_with("{connection_file}") {
            if !literal.is_empty() {
                output.push(json!({"kind":"literal","value":std::mem::take(&mut literal)}));
            }
            output.push(json!({"kind":"connection-file"}));
            index += "{connection_file}".len();
            continue;
        }
        let boundary = index == 0
            || value[..index]
                .chars()
                .next_back()
                .is_some_and(|c| matches!(c, '=' | ':' | ';' | ' ' | '\t' | '\n' | '"' | '\''));
        // Repository recognition must never consume an active substitution.
        let next_connection = if connection {
            tail.find("{connection_file}").unwrap_or(tail.len())
        } else {
            tail.len()
        };
        let root_match = boundary
            .then(|| {
                roots
                    .iter()
                    .filter(|(_, root)| {
                        let root = root.to_str().unwrap();
                        root.len() <= next_connection
                            && tail.starts_with(root)
                            && (root == "/"
                                || tail[root.len()..].chars().next().is_none_or(|c| {
                                    c == '/'
                                        || matches!(c, ':' | ';' | ' ' | '\t' | '\n' | '"' | '\'')
                                }))
                    })
                    .min_by(|(a, ar), (b, br)| {
                        br.as_os_str()
                            .len()
                            .cmp(&ar.as_os_str().len())
                            .then(a.cmp(b))
                    })
            })
            .flatten();
        if let Some((id, root)) = root_match {
            let root_len = root.to_str().unwrap().len();
            let end = tail[root_len..]
                .find(|c: char| {
                    matches!(c, ':' | ';' | ' ' | '\t' | '\n' | '"' | '\'')
                        || (connection && c == '{')
                })
                .map_or(tail.len(), |n| n + root_len);
            let raw_suffix = &tail[root_len..end];
            let suffix = if root == Path::new("/") {
                raw_suffix
            } else {
                raw_suffix.strip_prefix('/').unwrap_or(raw_suffix)
            };
            let normalized = suffix.trim_end_matches('/');
            // Argument bytes are not file declarations. Keep non-normalized
            // suffixes literal so lexical cleanup cannot merge distinct commands.
            let (relative, remainder) = if normalized.is_empty() {
                (".".to_owned(), raw_suffix.to_owned())
            } else if crate::diagnostics::DiagnosticPath::try_from(normalized).is_ok() {
                (normalized.to_owned(), suffix[normalized.len()..].to_owned())
            } else {
                (".".to_owned(), raw_suffix.to_owned())
            };
            if !literal.is_empty() {
                output.push(json!({"kind":"literal","value":std::mem::take(&mut literal)}));
            }
            output.push(json!({"kind":"repository","repository":id,"path":relative}));
            if !remainder.is_empty() {
                output.push(json!({"kind":"literal","value":remainder}));
            }
            index += end;
        } else {
            let character = tail.chars().next().unwrap();
            literal.push(character);
            index += character.len_utf8();
        }
    }
    if !literal.is_empty() || output.is_empty() {
        output.push(json!({"kind":"literal","value":literal}));
    }
    Ok(output)
}

// Presentation metadata may use arbitrary JSON numbers; duplicate keys still fail.
struct DuplicateChecked(Value);
impl<'de> Deserialize<'de> for DuplicateChecked {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Checked;
        impl<'de> serde::de::Visitor<'de> for Checked {
            type Value = DuplicateChecked;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("duplicate-free JSON")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(
                    serde_json::Number::from_f64(v)
                        .ok_or_else(|| E::custom("invalid number"))?
                        .into(),
                ))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(DuplicateChecked(v.into()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(DuplicateChecked(v)) = a.next_element()? {
                    values.push(v);
                }
                Ok(DuplicateChecked(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if values.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate field"));
                    }
                    let DuplicateChecked(v) = a.next_value()?;
                    values.insert(k, v);
                }
                Ok(DuplicateChecked(Value::Object(values)))
            }
        }
        d.deserialize_any(Checked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segmentation_uses_longest_root_alias_tiebreaks_and_component_boundaries() {
        let roots = BTreeMap::from([
            ("outer".into(), PathBuf::from("/work")),
            ("z".into(), PathBuf::from("/work/repo")),
            ("a".into(), PathBuf::from("/work/repo")),
        ]);
        assert_eq!(segments("--file=/work/repo/data",false,&roots).unwrap(),json!([{"kind":"literal","value":"--file="},{"kind":"repository","repository":"a","path":"data"}]).as_array().unwrap().clone());
        assert_eq!(
            segments("/work/repository", false, &roots).unwrap(),
            json!([{"kind":"repository","repository":"outer","path":"repository"}])
                .as_array()
                .unwrap()
                .clone()
        );
        assert_eq!(
            segments("/working", false, &roots).unwrap(),
            json!([{"kind":"literal","value":"/working"}])
                .as_array()
                .unwrap()
                .clone()
        );
        assert_eq!(
            segments("{connection_file}", false, &roots).unwrap(),
            json!([{"kind":"literal","value":"{connection_file}"}])
                .as_array()
                .unwrap()
                .clone()
        );
        assert_eq!(segments("--file={connection_file}:suffix",true,&roots).unwrap(),json!([{"kind":"literal","value":"--file="},{"kind":"connection-file"},{"kind":"literal","value":":suffix"}]).as_array().unwrap().clone());
        assert_ne!(
            segments("/work/repo/a/{connection_file}", true, &roots).unwrap(),
            segments("/work/repo/a{connection_file}", true, &roots).unwrap()
        );
        let slash_root = BTreeMap::from([("all".into(), PathBuf::from("/"))]);
        assert_eq!(
            segments("/tmp/input", false, &slash_root).unwrap(),
            json!([{"kind":"repository","repository":"all","path":"tmp/input"}])
                .as_array()
                .unwrap()
                .clone()
        );
    }
}

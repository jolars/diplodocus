use super::*;
use crate::configuration::GitHubBundleRelease;
use crate::snapshots::Snapshot;
use crate::validation::{ContentAsset, DocumentIdentity, ResolvedDocument};
use sha2::{Digest, Sha256};

#[derive(Debug, Default)]
pub(crate) struct ImportedBundles {
    observations: Vec<(PathBuf, PathBuf, Fingerprint)>,
    pub(crate) documents: BTreeMap<DocumentIdentity, ResolvedDocument>,
    pub(crate) mounts: BTreeMap<String, String>,
    pub(crate) assets: Vec<ContentAsset>,
}

fn digest_hex(digest: Sha256) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(hex, "{byte:02x}").expect("write to string");
    }
    hex
}

fn bundle_digest(path: &Path) -> std::io::Result<Fingerprint> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(Fingerprint {
        algorithm: "sha256".into(),
        value: digest_hex(digest),
    })
}

fn failure(path: &Path, message: impl ToString) -> AssemblyError {
    AssemblyError::Bundle {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

pub(super) fn validate_export(config: &WorkspaceConfiguration) -> Result<(), AssemblyError> {
    if config.repositories.len() != 1
        || config.packages.len() != 1
        || !config.package_bundles.is_empty()
        || !config.content.is_empty()
        || !config.concepts.is_empty()
        || !config.relationships.is_empty()
        || !config.components.is_empty()
        || config.presentation != Default::default()
    {
        return Err(AssemblyError::Diagnostics(vec![diagnostic(
            DiagnosticCode::InvalidConfiguration,
            "Bundle export requires one repository and one package, with package settings only.",
        )]));
    }
    Ok(())
}

impl ImportedBundles {
    pub(super) async fn load(
        configuration: &WorkspaceConfiguration,
        directory: &Path,
        workspace: &mut Workspace,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<Self, AssemblyError> {
        let mut imports = Self::default();
        for declaration in &configuration.package_bundles {
            if let Some(mount) = &declaration.mount {
                imports
                    .mounts
                    .insert(declaration.package.clone(), mount.clone());
            }
            if declaration.local_override {
                continue;
            }
            let selected = if let Some(path) = &declaration.path {
                if !path.is_relative() {
                    return Err(failure(path, "expected a configuration-relative path"));
                }
                directory.join(path)
            } else {
                let release = declaration
                    .github_release
                    .as_ref()
                    .expect("validated bundle source");
                release
                    .retrieve(directory, &declaration.package, cancellation)
                    .await?
            };
            let release_url = declaration
                .github_release
                .as_ref()
                .map(GitHubBundleRelease::url);
            let label = release_url
                .as_ref()
                .map_or(selected.as_path(), |url| Path::new(url.as_str()));
            let path = fs::canonicalize(&selected).map_err(|error| failure(label, error))?;
            if !path.is_file() {
                return Err(failure(label, "expected a regular file"));
            }
            let digest = bundle_digest(&path).map_err(|error| failure(label, error))?;
            if let Some(release) = &declaration.github_release
                && !digest.value.eq_ignore_ascii_case(&release.sha256)
            {
                return Err(failure(label, "release artifact SHA-256 mismatch"));
            }
            let artifact = Snapshot::load_bundle(&path).map_err(|error| failure(label, error))?;
            if let Some(release) = &declaration.github_release {
                release
                    .verify(&artifact, &declaration.package)
                    .map_err(|error| failure(label, error))?;
            }
            let incoming = artifact.workspace();
            let (id, package) = incoming
                .packages
                .first_key_value()
                .expect("validated package bundle");
            if id != &declaration.package {
                return Err(failure(
                    label,
                    format!("expected package `{}`, found `{id}`", declaration.package),
                ));
            }
            if workspace.packages.contains_key(id) {
                return Err(failure(label, format!("duplicate package ID `{id}`")));
            }
            if workspace
                .packages
                .values()
                .any(|existing| existing.slug == package.slug)
            {
                return Err(failure(
                    label,
                    format!("duplicate package slug `{}`", package.slug),
                ));
            }
            if workspace.repositories.contains_key(&package.repository) {
                return Err(failure(
                    label,
                    format!("duplicate repository ID `{}`", package.repository),
                ));
            }
            for (page_id, page) in &incoming.pages {
                if workspace
                    .pages
                    .insert(page_id.clone(), page.clone())
                    .is_some()
                {
                    return Err(failure(&selected, format!("duplicate page ID `{page_id}`")));
                }
            }
            workspace.packages.insert(id.clone(), package.clone());
            workspace.repositories.extend(incoming.repositories.clone());
            workspace.provenance.extend(incoming.provenance.clone());
            workspace.diagnostics.extend(incoming.diagnostics.clone());
            imports.documents.extend(
                artifact
                    .documents()
                    .iter()
                    .cloned()
                    .map(|record| (record.document.clone(), record)),
            );
            imports.assets.extend(artifact.assets().values().cloned());
            imports.observations.push((selected, path, digest));
        }
        if let Some(selected) = &configuration.presentation.header.repositories {
            for repository in selected {
                if !workspace.repositories.contains_key(repository) {
                    return Err(AssemblyError::Diagnostics(vec![diagnostic(
                        DiagnosticCode::InvalidConfiguration,
                        format!("Unknown header repository `{repository}`."),
                    )]));
                }
            }
        }
        imports.revalidate()?;
        Ok(imports)
    }

    pub(super) fn input_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.observations
            .iter()
            .flat_map(|(selected, canonical, _)| [selected.clone(), canonical.clone()])
    }

    pub(super) fn revalidate(&self) -> Result<(), AssemblyError> {
        for (selected, canonical, digest) in &self.observations {
            if fs::canonicalize(selected)? != *canonical || bundle_digest(canonical)? != *digest {
                return Err(AssemblyError::InputsChanged);
            }
        }
        Ok(())
    }
}

impl GitHubBundleRelease {
    pub(crate) fn valid(&self) -> bool {
        let coordinates = self
            .repository
            .split_once('/')
            .is_some_and(|(owner, repo)| {
                [owner, repo].into_iter().all(|part| {
                    !matches!(part, "" | "." | "..")
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                })
            });
        coordinates
            && !self.tag.is_empty()
            && self
                .tag
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."))
            && !self
                .tag
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\\' | '%'))
            && self.asset.ends_with(".sqlite")
            && self
                .asset
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            && [(self.revision.as_str(), 40), (self.sha256.as_str(), 64)]
                .into_iter()
                .all(|(pin, len)| pin.len() == len && pin.bytes().all(|c| c.is_ascii_hexdigit()))
    }

    fn url(&self) -> url::Url {
        let mut url = url::Url::parse("https://github.com").expect("static GitHub URL");
        url.path_segments_mut()
            .expect("hierarchical GitHub URL")
            .extend(self.repository.split('/'))
            .extend(["releases", "download"])
            .extend(self.tag.split('/'))
            .push(&self.asset);
        url
    }

    fn verify(&self, artifact: &Snapshot, expected_package: &str) -> Result<(), String> {
        let workspace = artifact.workspace();
        let (id, package) = workspace
            .packages
            .first_key_value()
            .expect("validated bundle");
        if id != expected_package {
            return Err(format!(
                "expected package `{expected_package}`, found `{id}`"
            ));
        }
        let revision = workspace.repositories[&package.repository]
            .revision
            .as_deref();
        if !revision.is_some_and(|revision| revision.eq_ignore_ascii_case(&self.revision)) {
            return Err(format!(
                "expected source revision `{}`, found {:?}",
                self.revision, revision
            ));
        }
        Ok(())
    }

    async fn retrieve(
        &self,
        directory: &Path,
        package: &str,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<PathBuf, AssemblyError> {
        let url = self.url();
        let source = Path::new(url.as_str());
        let pin = self.sha256.to_ascii_lowercase();
        let cache = directory.join(".diplodocus/cache/package-bundles/sha256");
        let destination = cache.join(format!("{pin}.sqlite"));
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.is_file() => {
                let digest = bundle_digest(&destination).map_err(|error| failure(source, error))?;
                if digest.value == pin {
                    return Ok(destination);
                }
            }
            Ok(_) => return Err(failure(source, "expected a regular cache file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(source, error)),
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .referer(false)
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|error| failure(source, error.without_url()))?;
        tokio::select! {
            () = cancellation.as_mut() => Err(AssemblyError::Cancelled),
            result = self.download(&client, url.clone(), &destination, package) =>
                result.map(|()| destination).map_err(|error| failure(source, error)),
        }
    }

    async fn download(
        &self,
        client: &reqwest::Client,
        url: url::Url,
        destination: &Path,
        package: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::AsyncWriteExt;

        let mut response = client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(reqwest::Error::without_url)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("unexpected HTTP status {}", response.status()).into());
        }
        let cache = destination.parent().expect("cache parent");
        fs::create_dir_all(cache)?;
        let (file, temporary) = tempfile::NamedTempFile::new_in(cache)?.into_parts();
        let mut file = tokio::fs::File::from_std(file);
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            size += chunk.len() as u64;
            if size >= 2 * 1024 * 1024 * 1024 {
                return Err("release asset exceeds GitHub's size limit".into());
            }
            digest.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        if !digest_hex(digest).eq_ignore_ascii_case(&self.sha256) {
            return Err("release artifact SHA-256 mismatch".into());
        }
        self.verify(&Snapshot::load_bundle(&temporary)?, package)?;
        temporary.persist(destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve(bytes: Vec<u8>, length: usize) -> url::Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&bytes).await.unwrap();
        });
        url::Url::parse(&format!("http://{address}/bundle.sqlite")).unwrap()
    }

    #[tokio::test]
    async fn release_download_validates_before_cache_publication() {
        let root = tempfile::tempdir().unwrap();
        let revision = "a".repeat(40);
        fs::write(root.path().join("manifest"), "").unwrap();
        let config = root.path().join("producer.yaml");
        fs::write(&config, format!(
            "project: {{name: Test}}\nrepositories: [{{id: source, path: ., revision: {revision}}}]\npackages:\n  - {{id: example, name: Example, slug: example, ecosystem: cpp, repository: source, path: ., metadata-path: manifest, targets: []}}\n"
        )).unwrap();
        let sources = crate::assembly::assemble_workspace(config).unwrap();
        let resolved = crate::validation::resolve_workspace(&sources).unwrap();
        let artifact = root.path().join("bundle.sqlite");
        Snapshot::from_bundle(&sources, &resolved)
            .unwrap()
            .publish(&artifact)
            .unwrap();
        let bytes = fs::read(artifact).unwrap();
        let len = bytes.len();
        let release = GitHubBundleRelease {
            repository: "owner/repo".into(),
            tag: "v1".into(),
            asset: "bundle.sqlite".into(),
            revision,
            sha256: fingerprint_bytes(&bytes).value,
        };
        assert_eq!(
            release.url().as_str(),
            "https://github.com/owner/repo/releases/download/v1/bundle.sqlite"
        );

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cache = root.path().join("cache/bundle.sqlite");
        let first = serve(bytes.clone(), len).await;
        let second = serve(bytes.clone(), len).await;
        let (a, b) = tokio::join!(
            release.download(&client, first, &cache, "example"),
            release.download(&client, second, &cache, "example"),
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(fs::read(&cache).unwrap(), bytes);

        fs::write(&cache, b"corrupt cache").unwrap();
        let repair = serve(bytes.clone(), len).await;
        release
            .download(&client, repair, &cache, "example")
            .await
            .unwrap();
        assert_eq!(fs::read(&cache).unwrap(), bytes);

        for (body, length, package, expected) in [
            (b"wrong bytes".to_vec(), 11, "example", "SHA-256"),
            (bytes.clone(), len, "wrong", "expected package"),
            (bytes.clone(), len + 1, "example", "response body"),
        ] {
            let url = serve(body, length).await;
            let error = release
                .download(&client, url, &cache, package)
                .await
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        let wrong_revision = GitHubBundleRelease {
            revision: "b".repeat(40),
            ..release
        };
        let url = serve(bytes.clone(), len).await;
        assert!(
            wrong_revision
                .download(&client, url, &cache, "example")
                .await
                .unwrap_err()
                .to_string()
                .contains("source revision")
        );
        assert_eq!(fs::read(&cache).unwrap(), bytes);
        assert_eq!(fs::read_dir(cache.parent().unwrap()).unwrap().count(), 1);
    }
}

use super::*;
use crate::snapshots::Snapshot;
use crate::validation::{ContentAsset, DocumentIdentity, ResolvedDocument};

#[derive(Debug, Default)]
pub(crate) struct ImportedBundles {
    observations: Vec<(PathBuf, PathBuf, Fingerprint)>,
    pub(crate) documents: BTreeMap<DocumentIdentity, ResolvedDocument>,
    pub(crate) assets: Vec<ContentAsset>,
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
    pub(super) fn load(
        configuration: &WorkspaceConfiguration,
        directory: &Path,
        workspace: &mut Workspace,
    ) -> Result<Self, AssemblyError> {
        let mut imports = Self::default();
        for declaration in &configuration.package_bundles {
            let selected = directory.join(&declaration.path);
            if !declaration.path.is_relative() {
                return Err(failure(&selected, "expected a configuration-relative path"));
            }
            let path = fs::canonicalize(&selected).map_err(|error| failure(&selected, error))?;
            if !path.is_file() {
                return Err(failure(&selected, "expected a regular file"));
            }
            let digest = crate::incremental::file_digest(&path)
                .map_err(|error| failure(&selected, error))?;
            let artifact =
                Snapshot::load_bundle(&path).map_err(|error| failure(&selected, error))?;
            let incoming = artifact.workspace();
            let (id, package) = incoming
                .packages
                .first_key_value()
                .expect("validated package bundle");
            if id != &declaration.package {
                return Err(failure(
                    &selected,
                    format!("expected package `{}`, found `{id}`", declaration.package),
                ));
            }
            if workspace.packages.contains_key(id) {
                return Err(failure(&selected, format!("duplicate package ID `{id}`")));
            }
            if workspace
                .packages
                .values()
                .any(|existing| existing.slug == package.slug)
            {
                return Err(failure(
                    &selected,
                    format!("duplicate package slug `{}`", package.slug),
                ));
            }
            if workspace.repositories.contains_key(&package.repository) {
                return Err(failure(
                    &selected,
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
            if fs::canonicalize(selected)? != *canonical
                || crate::incremental::file_digest(canonical)? != *digest
            {
                return Err(AssemblyError::InputsChanged);
            }
        }
        Ok(())
    }
}

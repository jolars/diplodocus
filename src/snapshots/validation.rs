use super::*;
use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::Severity;
use crate::execution::assets::validate_authored_image_bytes;
use crate::ir::*;
use crate::provenance::fingerprint_bytes;
use crate::validation::{
    DocumentIdentity, ReferenceKind, ReferenceTarget, document_anchors, document_references,
    resolve_item,
};

mod links;

pub(super) fn require(condition: bool, message: &'static str) -> Result<(), SnapshotError> {
    condition
        .then_some(())
        .ok_or(SnapshotError::Invalid(message))
}

impl Snapshot {
    pub(super) fn validate(
        &self,
    ) -> Result<BTreeMap<String, crate::execution::ValidatedPage>, SnapshotError> {
        self.presentation
            .site_base_url()
            .map_err(SnapshotError::Invalid)?;
        links::validate(&self.workspace)?;
        let outputs = self.restore_outputs()?;
        let workspace = &self.workspace;
        require(!self.producer.is_empty(), "missing producer")?;
        require(
            !workspace
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error),
            "error diagnostic in successful snapshot",
        )?;
        let mut expected_documents = BTreeMap::new();
        let owns = |owner: &ContentOwner| match owner {
            ContentOwner::Project => true,
            ContentOwner::Package { package } => workspace.packages.contains_key(package),
        };
        for (id, collection) in &workspace.content_collections {
            require(
                !id.is_empty()
                    && workspace.repositories.contains_key(&collection.repository)
                    && owns(&collection.owner),
                "collection identity or owner",
            )?;
        }
        let mut slugs = BTreeSet::new();
        for (id, package) in &workspace.packages {
            require(
                !id.is_empty()
                    && workspace.repositories.contains_key(&package.repository)
                    && slugs.insert(&package.slug),
                "package identity or repository",
            )?;
            for (item_id, item) in &package.items {
                require(!item_id.is_empty(), "empty item identity")?;
                require(
                    item.children
                        .iter()
                        .all(|child| package.items.contains_key(child)),
                    "missing child item",
                )?;
                if let Some(source) = &item.source_location {
                    require(
                        workspace.repositories.contains_key(&source.repository),
                        "missing item source repository",
                    )?;
                }
                if let Some(document) = &item.documentation {
                    expected_documents.insert(
                        DocumentIdentity::Item {
                            item: ItemReference {
                                package: id.clone(),
                                item: item_id.clone(),
                            },
                        },
                        (document, Some(id.as_str())),
                    );
                }
            }
        }
        let item_exists = |item: &ItemReference| {
            workspace
                .packages
                .get(&item.package)
                .is_some_and(|p| p.items.contains_key(&item.item))
        };
        for (id, concept) in &workspace.concepts {
            require(
                !id.is_empty() && concept.members.iter().all(item_exists),
                "concept member identity",
            )?;
            if let Some(document) = &concept.documentation {
                expected_documents.insert(
                    DocumentIdentity::Concept {
                        concept: id.clone(),
                    },
                    (document, None),
                );
            }
        }
        for relationship in &workspace.relationships {
            for endpoint in [&relationship.from, &relationship.to] {
                if let PackageReference::Workspace { package } = endpoint {
                    require(
                        workspace.packages.contains_key(package),
                        "relationship endpoint",
                    )?;
                }
            }
        }
        let mut package_page_roles = BTreeSet::new();
        for (id, page) in &workspace.pages {
            require(
                !id.is_empty() && owns(&page.owner),
                "page identity or owner",
            )?;
            match &page.kind {
                PageKind::Authored { collection } => require(
                    workspace
                        .content_collections
                        .get(collection)
                        .is_some_and(|c| c.owner == page.owner),
                    "page collection",
                )?,
                PageKind::Api { item } => require(item_exists(item), "API page item")?,
                PageKind::Concept { concept } => {
                    require(workspace.concepts.contains_key(concept), "concept page")?
                }
                PageKind::PackageDocumentation { role } => {
                    let ContentOwner::Package { package } = &page.owner else {
                        return Err(SnapshotError::Invalid("package documentation owner"));
                    };
                    require(
                        matches!(role, PackagePageRole::Overview | PackagePageRole::License)
                            || matches!(
                                workspace.packages[package].metadata,
                                Some(PackageMetadata::R(_))
                            ),
                        "package documentation owner",
                    )?;
                    require(
                        package_page_roles.insert((package, role)),
                        "duplicate package documentation role",
                    )?;
                }
                PageKind::Overview => {}
            }
            let owner = match &page.owner {
                ContentOwner::Project => None,
                ContentOwner::Package { package } => Some(package.as_str()),
            };
            expected_documents.insert(
                DocumentIdentity::Page { page: id.clone() },
                (&page.document, owner),
            );
        }
        let records: BTreeMap<_, _> = self
            .documents
            .iter()
            .map(|r| (r.document.clone(), r))
            .collect();
        require(
            records.len() == self.documents.len() && records.keys().eq(expected_documents.keys()),
            "document record set",
        )?;
        let mut used_assets = BTreeSet::new();
        require(
            self.presentation.open_graph.is_some() == self.open_graph.is_some(),
            "Open Graph presentation evidence",
        )?;
        if let (Some(declaration), Some(image)) = (&self.presentation.open_graph, &self.open_graph)
        {
            require(
                self.presentation.site_url.is_some()
                    && !declaration.alt.trim().is_empty()
                    && workspace.repositories.contains_key(&declaration.repository),
                "Open Graph presentation settings",
            )?;
            let stored = self
                .assets
                .get(&image.fingerprint.value)
                .ok_or(SnapshotError::Invalid("missing Open Graph asset"))?;
            require(
                stored.fingerprint == image.fingerprint
                    && matches!(stored.media_type.as_str(), "image/png" | "image/jpeg")
                    && image.path.as_str()
                        == format!("content-assets/sha256/{}", image.fingerprint.value),
                "Open Graph asset metadata",
            )?;
            used_assets.insert(image.fingerprint.value.clone());
        }
        require(
            self.presentation.logo.is_some() == self.logo.is_some(),
            "logo presentation evidence",
        )?;
        if let (Some(declaration), Some(logo)) = (&self.presentation.logo, &self.logo) {
            require(
                workspace.repositories.contains_key(&declaration.repository),
                "logo repository",
            )?;
            let stored = self
                .assets
                .get(&logo.fingerprint.value)
                .ok_or(SnapshotError::Invalid("missing logo asset"))?;
            require(
                stored.fingerprint == logo.fingerprint
                    && stored.media_type.starts_with("image/")
                    && logo.path.as_str()
                        == format!("content-assets/sha256/{}", logo.fingerprint.value),
                "logo asset metadata",
            )?;
            used_assets.insert(logo.fingerprint.value.clone());
        }
        let generated: BTreeMap<_, _> = outputs
            .values()
            .flat_map(|p| p.referenced_assets())
            .map(|a| {
                used_assets.insert(a.reference.fingerprint.value.clone());
                (a.reference.path.clone(), a)
            })
            .collect();
        for (identity, (document, owner)) in expected_documents {
            if let Some(source) = &document.source_location {
                require(
                    workspace.repositories.contains_key(&source.repository),
                    "document source repository",
                )?;
            }
            let record = records[&identity];
            let authored = matches!(&identity, DocumentIdentity::Page { page } if matches!(workspace.pages[page].kind, PageKind::Authored { .. }));
            require(
                record.collection_path.is_some() == authored,
                "collection-relative document path",
            )?;
            require(
                record.anchors == document_anchors(&document.document.blocks),
                "document anchors",
            )?;
            let mut references = Vec::new();
            document_references(
                &document.document.blocks,
                None,
                &mut |kind, spelling, _, _| references.push((kind, spelling.to_owned())),
            );
            require(
                references.len() == record.references.len(),
                "reference count",
            )?;
            for ((kind, spelling), reference) in references.into_iter().zip(&record.references) {
                require(
                    kind == reference.kind && spelling == reference.spelling,
                    "reference association",
                )?;
                let rd_help = kind == ReferenceKind::Link
                    && matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "rd")
                    && spelling.starts_with("@ref ");
                if rd_help {
                    require(
                        crate::validation::resolve_rd_help(workspace, owner, &spelling[5..])
                            .as_ref()
                            == Ok(&reference.target),
                        "Rd help reference",
                    )?;
                    continue;
                }
                require(
                    crate::validation::semantic_target(document, kind, &spelling).is_none()
                        || matches!(&reference.target, ReferenceTarget::Item { .. }),
                    "semantic reference destination",
                )?;
                match &reference.target {
                    ReferenceTarget::Item { item } => {
                        require(
                            crate::validation::semantic_target(document, kind, &spelling)
                                .is_some_and(|target| {
                                    resolve_item(workspace, owner, target).as_ref() == Ok(item)
                                }),
                            "semantic reference",
                        )?;
                    }
                    ReferenceTarget::Page { page, fragment } => {
                        require(
                            kind == ReferenceKind::Link && workspace.pages.contains_key(page),
                            "page reference",
                        )?;
                        if let Some(fragment) = fragment
                            && !fragment.is_empty()
                        {
                            require(
                                records[&DocumentIdentity::Page { page: page.clone() }]
                                    .anchors
                                    .contains(fragment),
                                "page anchor",
                            )?;
                        }
                    }
                    ReferenceTarget::Anchor { fragment } => require(
                        kind == ReferenceKind::Link && record.anchors.contains(fragment),
                        "local anchor",
                    )?,
                    ReferenceTarget::Asset { asset, .. } => {
                        require(kind != ReferenceKind::Semantic, "asset reference kind")?;
                        let stored = self
                            .assets
                            .get(&asset.fingerprint.value)
                            .ok_or(SnapshotError::Invalid("missing referenced asset"))?;
                        require(
                            stored.fingerprint == asset.fingerprint,
                            "asset digest metadata",
                        )?;
                        require(
                            asset.path.as_str()
                                == format!("content-assets/sha256/{}", asset.fingerprint.value)
                                || generated
                                    .get(&asset.path)
                                    .is_some_and(|a| a.reference == *asset),
                            "asset path",
                        )?;
                        if kind == ReferenceKind::Image {
                            require(stored.media_type.starts_with("image/"), "image media type")?;
                        }
                        used_assets.insert(asset.fingerprint.value.clone());
                    }
                    ReferenceTarget::Repository {
                        repository,
                        fragment,
                        ..
                    } => {
                        let decoded = percent_encoding::percent_decode_str(&spelling)
                            .decode_utf8()
                            .map_err(|_| SnapshotError::Invalid("repository hyperlink spelling"))?;
                        let authored_fragment = spelling
                            .split_once('#')
                            .map(|(_, fragment)| {
                                percent_encoding::percent_decode_str(fragment)
                                    .decode_utf8()
                                    .map(|fragment| fragment.into_owned())
                            })
                            .transpose()
                            .map_err(|_| SnapshotError::Invalid("repository hyperlink fragment"))?;
                        require(
                            kind == ReferenceKind::Link
                                && matches!(&identity, DocumentIdentity::Page { page }
                                    if matches!(workspace.pages[page].kind, PageKind::PackageDocumentation { .. }))
                                && document
                                    .source_location
                                    .as_ref()
                                    .is_some_and(|source| source.repository == *repository)
                                && !spelling.is_empty()
                                && !decoded.starts_with('/')
                                && !spelling.starts_with('#')
                                && !decoded.chars().any(|c| c.is_control() || c == '\\')
                                && !spelling.contains('?')
                                && url::Url::parse(&spelling).is_err()
                                && url::Url::parse(&decoded).is_err()
                                && fragment
                                    .as_ref()
                                    .is_none_or(|fragment| !fragment.chars().any(char::is_control))
                                && fragment == &authored_fragment
                                && crate::validation::repository_target_url(
                                    workspace,
                                    &reference.target,
                                )
                                .is_some(),
                            "repository hyperlink policy",
                        )?;
                    }
                    ReferenceTarget::External { url } => {
                        let parsed = url::Url::parse(url)
                            .map_err(|_| SnapshotError::Invalid("external URL"))?;
                        require(
                            kind == ReferenceKind::Link
                                && *url == spelling
                                && matches!(parsed.scheme(), "http" | "https" | "mailto")
                                && parsed.username().is_empty()
                                && parsed.password().is_none()
                                && !url.chars().any(char::is_control),
                            "external URL policy",
                        )?;
                    }
                }
            }
        }
        for (digest, asset) in &self.assets {
            require(
                asset.fingerprint == fingerprint_bytes(&asset.bytes)
                    && *digest == asset.fingerprint.value,
                "asset bytes",
            )?;
            if asset.media_type != "application/octet-stream" {
                validate_authored_image_bytes(&asset.media_type, &asset.bytes)
                    .map_err(|_| SnapshotError::Invalid("asset media"))?;
            }
        }
        require(
            used_assets.iter().eq(self.assets.keys()),
            "unreferenced asset",
        )?;
        Ok(outputs)
    }
}

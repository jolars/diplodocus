use super::*;

impl Resolver<'_> {
    pub(super) fn repository_link(
        &mut self,
        context: &DocumentContext<'_>,
        kind: ReferenceKind,
        candidate: PathBuf,
        fragment: Option<String>,
    ) -> Result<ReferenceTarget, DiagnosticCode> {
        let invalid = DiagnosticCode::UnresolvedDocumentReference;
        let DocumentIdentity::Page { page } = &context.identity else {
            return Err(invalid);
        };
        if kind != ReferenceKind::Link
            || !matches!(
                self.sources.workspace().pages[page].kind,
                PageKind::PackageDocumentation { .. }
            )
        {
            return Err(invalid);
        }
        let source = context.document.source_location.as_ref().ok_or(invalid)?;
        let repository = self
            .sources
            .paths()
            .repositories
            .iter()
            .find(|repository| repository.id == source.repository)
            .ok_or(invalid)?;
        // Check the original traversal so a symlink cannot leave and reenter the root.
        let path = crate::paths::resolve_repository_target(
            candidate
                .strip_prefix(&repository.path)
                .map_err(|_| invalid)?,
            &repository.path,
        )
        .map_err(|_| invalid)?;
        let directory = path.is_dir();
        let relative = crate::paths::portable_relative_path(
            &repository.path,
            "repository hyperlink",
            path.strip_prefix(&repository.path).map_err(|_| invalid)?,
        )
        .map_err(|_| invalid)?;
        let target = ReferenceTarget::Repository {
            repository: repository.id.clone(),
            path: relative,
            directory,
            fragment,
        };
        repository_target_url(self.sources.workspace(), &target).ok_or(invalid)?;
        let observation = RepositoryTargetObservation {
            path,
            boundary: repository.path.clone(),
            directory,
        };
        if let Some(previous) = self.result.repository_targets.get(&candidate)
            && (previous.path != observation.path || previous.directory != observation.directory)
        {
            return Err(invalid);
        }
        self.result
            .repository_targets
            .insert(candidate, observation);
        Ok(target)
    }
}

pub(crate) fn repository_target_url(
    workspace: &Workspace,
    target: &ReferenceTarget,
) -> Option<String> {
    let ReferenceTarget::Repository {
        repository,
        path,
        directory,
        fragment,
    } = target
    else {
        return None;
    };
    source_url(
        workspace.repositories.get(repository)?,
        path.as_str(),
        *directory,
        fragment.as_deref(),
    )
}

fn source_url(
    repository: &Repository,
    path: &str,
    directory: bool,
    fragment: Option<&str>,
) -> Option<String> {
    let revision = repository.revision.as_deref().filter(|s| !s.is_empty())?;
    let kind = if directory { "tree" } else { "blob" };
    let mut url = if let Some(template) = &repository.source_link_template {
        if !template.contains("{revision}")
            || !template.contains("{path}")
            || (directory && !template.contains("{kind}"))
        {
            return None;
        }
        let expanded = template
            .replace("{revision}", &encode_segment(revision))
            .replace(
                "{path}",
                &path
                    .split('/')
                    .map(encode_segment)
                    .collect::<Vec<_>>()
                    .join("/"),
            )
            .replace("{kind}", kind)
            .replace("{line}", "1");
        if expanded.contains(['{', '}']) {
            return None;
        }
        url::Url::parse(&expanded).ok()?
    } else {
        let mut url = url::Url::parse(repository.canonical_url.as_deref()?).ok()?;
        if url.host_str() != Some("github.com") || url.query().is_some() || url.fragment().is_some()
        {
            return None;
        }
        let components: Vec<_> = url.path().trim_matches('/').split('/').collect();
        let [owner, name] = components.as_slice() else {
            return None;
        };
        let owner = owner.to_string();
        let name = name.strip_suffix(".git").unwrap_or(name).to_string();
        if owner.is_empty() || name.is_empty() {
            return None;
        }
        url.set_path(&format!("/{owner}/{name}"));
        let mut segments = url.path_segments_mut().ok()?;
        segments.push(kind).push(revision);
        for segment in path.split('/') {
            segments.push(segment);
        }
        drop(segments);
        url
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return None;
    }
    // Whole-file links do not invent line evidence from a template's line suffix.
    url.set_fragment(fragment.map(encode_segment).as_deref());
    Some(url.into())
}

fn encode_segment(value: &str) -> String {
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(value, SEGMENT).to_string()
}

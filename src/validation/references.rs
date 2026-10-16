use crate::diagnostics::DiagnosticCode;
use crate::ir::{
    ItemLanguageData, ItemReference, JuliaCallableRole, JuliaDeclaration, PythonCallableRole,
    PythonDeclaration, Workspace,
};

/// Resolve a semantic name without assigning or consulting a rendered route.
///
/// Qualified names select their explicit package. Unqualified names prefer the
/// owner, then require a unique workspace match. Callable names select families;
/// a specific overload remains addressable by its exact semantic ID.
pub fn resolve_item(
    workspace: &Workspace,
    owner: Option<&str>,
    target: &str,
) -> Result<ItemReference, DiagnosticCode> {
    if let Some((package, name)) = target.split_once("::")
        && !package.contains(['(', '{', '['])
        && workspace.packages.contains_key(package)
    {
        match in_package(workspace, package, name) {
            Err(DiagnosticCode::UnresolvedItemReference)
                if workspace.packages[package].ecosystem == "rust" =>
            {
                return in_package(workspace, package, target);
            }
            result => return result,
        }
    }
    if let Some(owner) = owner {
        match in_package(workspace, owner, target) {
            Err(DiagnosticCode::UnresolvedItemReference) => {}
            result => return result,
        }
    }
    let mut found = None;
    for package in workspace.packages.keys() {
        match in_package(workspace, package, target) {
            Ok(item) if found.is_none() => found = Some(item),
            Ok(_) | Err(DiagnosticCode::AmbiguousItemReference) => {
                return Err(DiagnosticCode::AmbiguousItemReference);
            }
            Err(_) => {}
        }
    }
    found.ok_or(DiagnosticCode::UnresolvedItemReference)
}

pub(crate) fn in_package(
    workspace: &Workspace,
    package: &str,
    name: &str,
) -> Result<ItemReference, DiagnosticCode> {
    let items = &workspace
        .packages
        .get(package)
        .ok_or(DiagnosticCode::UnresolvedItemReference)?
        .items;
    let found = if items.contains_key(name) {
        name
    } else if workspace.packages[package].ecosystem == "julia"
        && let Some((family_name, dispatch)) = crate::extractors::julia::method_selector(name)
    {
        let family = in_package(workspace, package, &family_name)?;
        let (id, _) = items.iter().find(|(_, item)| {
            matches!(&item.language_data, Some(ItemLanguageData::Julia(data)) if matches!(&data.declaration,
                JuliaDeclaration::Callable { role: JuliaCallableRole::Method { family: target, dispatch: actual }, .. }
                if target == &family && actual == &dispatch))
        }).ok_or(DiagnosticCode::UnresolvedItemReference)?;
        id
    } else {
        let mut candidates = items.iter().filter(|(_, item)| {
            !matches!(&item.language_data, Some(ItemLanguageData::Python(data)) if matches!(&data.declaration,
                PythonDeclaration::Callable { role: PythonCallableRole::Overload { .. }, .. }))
            && !matches!(&item.language_data, Some(ItemLanguageData::Julia(data)) if matches!(&data.declaration,
                JuliaDeclaration::Callable { role: JuliaCallableRole::Method { .. }, .. }))
            && (item.qualified_name == name || item.aliases.iter().any(|a| a.qualified_name == name)
                || (workspace.packages[package].ecosystem == "julia" && !name.contains('.')
                    && (item.qualified_name.rsplit('.').next() == Some(name)
                        || item.aliases.iter().any(|alias| alias.qualified_name.rsplit('.').next() == Some(name))))
                || matches!(&item.language_data, Some(ItemLanguageData::Rust(data))
                    if data.public_path == name
                        || data.public_path.split_once("::").is_some_and(|(_, rest)| rest == name)
                        || item.qualified_name.split_once("::").is_some_and(|(_, rest)| rest == name)
                        || item.aliases.iter().any(|alias| alias.qualified_name.split_once("::").is_some_and(|(_, rest)| rest == name))))
        });
        let (id, _) = candidates
            .next()
            .ok_or(DiagnosticCode::UnresolvedItemReference)?;
        if candidates.next().is_some() {
            return Err(DiagnosticCode::AmbiguousItemReference);
        }
        id
    };
    Ok(ItemReference {
        package: package.into(),
        item: found.into(),
    })
}

/// Resolve Rd topic coordinates from workspace aliases and retained installed help evidence.
pub(crate) fn resolve_rd_help(
    workspace: &Workspace,
    owner: Option<&str>,
    spelling: &str,
) -> Result<super::ReferenceTarget, DiagnosticCode> {
    use super::ReferenceTarget;
    use crate::ir::PackageMetadata;
    let (qualifier, topic) = spelling
        .split_once("::")
        .map_or((None, spelling), |(package, topic)| (Some(package), topic));
    if topic.is_empty()
        || spelling.chars().any(char::is_control)
        || qualifier.is_some_and(str::is_empty)
    {
        return Err(DiagnosticCode::UnresolvedItemReference);
    }
    let matches_package = |id: &str, package: &crate::ir::Package| {
        package.ecosystem == "r" && qualifier.is_none_or(|name| {
            id == name || matches!(&package.metadata, Some(PackageMetadata::R(metadata)) if metadata.description.name == name)
        })
    };
    if qualifier.is_none()
        && let Some(owner) = owner
    {
        match in_package(workspace, owner, topic) {
            Ok(item) => return Ok(ReferenceTarget::Item { item }),
            Err(DiagnosticCode::UnresolvedItemReference) => {}
            Err(code) => return Err(code),
        }
    }
    let mut found = None;
    let mut workspace_package = false;
    for (id, package) in &workspace.packages {
        if !matches_package(id, package) {
            continue;
        }
        workspace_package = true;
        match in_package(workspace, id, topic) {
            Ok(item) if found.is_none() => found = Some(item),
            Ok(_) | Err(DiagnosticCode::AmbiguousItemReference) => {
                return Err(DiagnosticCode::AmbiguousItemReference);
            }
            Err(_) => {}
        }
    }
    if let Some(item) = found {
        return Ok(ReferenceTarget::Item { item });
    }
    // A workspace package owns its help surface even when an installed release differs.
    if qualifier.is_some() && workspace_package {
        return Err(DiagnosticCode::UnresolvedItemReference);
    }
    let candidates = owner
        .and_then(|owner| workspace.packages.get(owner))
        .and_then(|package| match &package.metadata {
            Some(PackageMetadata::R(metadata)) => metadata.help_links.get(spelling),
            _ => None,
        });
    let Some(candidates) = candidates else {
        return Err(DiagnosticCode::UnresolvedItemReference);
    };
    match candidates.as_slice() {
        [target] => Ok(ReferenceTarget::External {
            url: rdrr_url(target),
        }),
        [] => Err(DiagnosticCode::UnresolvedItemReference),
        _ => Err(DiagnosticCode::AmbiguousItemReference),
    }
}

pub(crate) fn rdrr_url(target: &crate::ir::RHelpTarget) -> String {
    use crate::ir::RdrrRepository;
    let path = match target.repository {
        RdrrRepository::R => format!("https://rdrr.io/r/{}/", target.package),
        RdrrRepository::Cran => format!("https://rdrr.io/cran/{}/man/", target.package),
        RdrrRepository::Bioc => format!("https://rdrr.io/bioc/{}/man/", target.package),
    };
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    format!(
        "{path}{}.html",
        percent_encoding::utf8_percent_encode(&target.topic, SEGMENT)
    )
}

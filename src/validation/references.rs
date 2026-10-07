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
    {
        return in_package(workspace, package, name);
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
            && (item.qualified_name == name || item.aliases.iter().any(|a| a.qualified_name == name))
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

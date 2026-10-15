//! Reconcile native type observations with source declarations.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    policy: String,
    julia_version: String,
    package_name: String,
    package_uuid: String,
    package_version: String,
    dependencies: BTreeMap<String, String>,
    types: Vec<Type>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Type {
    name: String,
    qualified_name: String,
    supertype: String,
    #[serde(rename = "abstract")]
    is_abstract: bool,
    mutable: bool,
    parameters: Vec<String>,
    fields: Vec<Field>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    name: String,
    #[serde(rename = "type")]
    annotation: String,
}

fn expression(name: &str) -> SignatureExpression {
    SignatureExpression::Name {
        name: name.into(),
        target: None,
    }
}

pub(crate) fn reconcile(
    result: &mut JuliaExtraction,
    observation: Observation,
) -> Result<(), String> {
    let metadata = result
        .metadata
        .as_ref()
        .ok_or("Julia metadata is unavailable.")?;
    if observation.policy != "julia-runtime-v1"
        || observation.package_name != metadata.name
        || observation.package_uuid != metadata.uuid
        || metadata
            .version
            .as_ref()
            .is_some_and(|v| v != &observation.package_version)
        || observation.julia_version.is_empty()
        || observation.dependencies.is_empty()
    {
        return Err("Julia runtime identity conflicts with Project.toml.".into());
    }
    let provenance = Provenance {
        activity: ProvenanceActivity::JuliaRuntime {
            target: result.target.clone(),
            policy: observation.policy,
            julia_version: observation.julia_version.clone(),
            package_name: observation.package_name.clone(),
            package_uuid: observation.package_uuid,
            package_version: observation.package_version,
            dependencies: observation.dependencies,
        },
        source: None,
        span: None,
        tools: BTreeMap::from([("julia".into(), observation.julia_version)]),
    };
    let mut observed = BTreeSet::new();
    let mut generated = Vec::new();
    for value in observation.types {
        if value.qualified_name != format!("{}.{}", observation.package_name, value.name)
            || !observed.insert(value.qualified_name.clone())
        {
            return Err("Invalid or duplicate Julia runtime type identity.".into());
        }
        let id = SemanticIdentity::julia(&value.qualified_name, JuliaIdentityKind::Type)
            .map_err(|e| e.to_string())?
            .item_id()
            .to_owned();
        let flavor = if value.is_abstract {
            JuliaTypeKind::Abstract
        } else {
            JuliaTypeKind::Struct {
                mutable: value.mutable,
            }
        };
        if let Some(item) = result.items.get_mut(&id) {
            let Some(ItemLanguageData::Julia(data)) = &item.language_data else {
                return Err(format!(
                    "Julia runtime type conflicts with `{}`.",
                    value.qualified_name
                ));
            };
            if !matches!(&data.declaration, JuliaDeclaration::Type { flavor: source_flavor, .. } if *source_flavor == flavor)
            {
                return Err(format!(
                    "Julia runtime type flavor conflicts with `{}`.",
                    value.qualified_name
                ));
            }
            item.provenance.push(provenance.clone());
            continue;
        }
        if result
            .items
            .values()
            .any(|item| item.qualified_name == value.qualified_name)
        {
            return Err(format!(
                "Julia runtime type conflicts with `{}`.",
                value.qualified_name
            ));
        }
        let mut item = Item {
            kind: ItemKind::Type,
            name: value.name,
            qualified_name: value.qualified_name.clone(),
            language_data: Some(ItemLanguageData::Julia(JuliaItemData {
                defining_module: observation.package_name.clone(),
                visibility: JuliaVisibility::Exported,
                declaration: JuliaDeclaration::Type {
                    flavor,
                    parameters: value.parameters.iter().map(|p| expression(p)).collect(),
                    supertype: Some(expression(&value.supertype)),
                    constructors: Vec::new(),
                },
            })),
            aliases: Vec::new(),
            signatures: vec![SourcedSignature {
                signature: Signature::LanguageSpecific {
                    language: "julia".into(),
                    syntax: SignatureExpression::LanguageSpecific {
                        language: "julia".into(),
                        name: "runtime-subtype".into(),
                        children: vec![
                            expression(&value.qualified_name),
                            SignatureExpression::Literal { text: "<:".into() },
                            expression(&value.supertype),
                        ],
                        source: None,
                    },
                },
                sources: Vec::new(),
            }],
            documentation: None,
            source_location: None,
            children: Vec::new(),
            provenance: vec![provenance.clone()],
        };
        for field in value.fields {
            let name = format!("{}.{}", value.qualified_name, field.name);
            let field_id = SemanticIdentity::julia(&name, JuliaIdentityKind::Field)
                .map_err(|e| e.to_string())?
                .item_id()
                .to_owned();
            item.children.push(field_id.clone());
            result.items.insert(
                field_id,
                Item {
                    kind: ItemKind::Field,
                    name: field.name,
                    qualified_name: name,
                    language_data: Some(ItemLanguageData::Julia(JuliaItemData {
                        defining_module: observation.package_name.clone(),
                        visibility: JuliaVisibility::Private,
                        declaration: JuliaDeclaration::Field { is_const: false },
                    })),
                    aliases: Vec::new(),
                    signatures: vec![SourcedSignature {
                        signature: Signature::Value {
                            annotation: Some(expression(&field.annotation)),
                            value: None,
                        },
                        sources: Vec::new(),
                    }],
                    documentation: None,
                    source_location: None,
                    children: Vec::new(),
                    provenance: vec![provenance.clone()],
                },
            );
        }
        generated.push(id.clone());
        result.items.insert(id, item);
    }
    result.diagnostics.retain(|diagnostic| {
        diagnostic.code != DiagnosticCode::JuliaUnresolvedDefinition
            || !observed.iter().any(|name| {
                diagnostic.message
                    == format!("Public Julia binding `{name}` has no supplied static definition.")
            })
    });
    if let Some(root) = result.items.values_mut().find(|item| {
        item.qualified_name == observation.package_name && item.kind == ItemKind::Module
    }) {
        root.children.extend(generated);
        root.provenance.push(provenance);
    }
    Ok(())
}

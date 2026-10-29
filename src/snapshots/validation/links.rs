use super::require;
use crate::diagnostics::{DiagnosticPath, DiagnosticSource};
use crate::ir::*;
use crate::snapshots::SnapshotError;

type Result = std::result::Result<(), SnapshotError>;

/// Check semantic joins before consumers traverse untrusted nested records.
pub(super) fn validate(workspace: &Workspace) -> Result {
    let links = Links(workspace);
    for id in workspace.repositories.keys() {
        require(!id.is_empty(), "empty repository identity")?;
    }
    links.provenance(&workspace.provenance)?;
    for diagnostic in &workspace.diagnostics {
        if let Some(source) = &diagnostic.source {
            links.diagnostic_source(source)?;
        }
    }
    for (package_id, package) in &workspace.packages {
        route(&package.slug)?;
        if let Some(metadata) = &package.metadata {
            let root = package.path.as_ref().map_or("", DiagnosticPath::as_str);
            let path = std::path::Path::new(root).join(package.metadata_path.as_str());
            let ecosystem = match metadata {
                PackageMetadata::R(_) => "r",
                PackageMetadata::Python(_) => "python",
                PackageMetadata::Julia(_) => "julia",
                PackageMetadata::Rust(_) => "rust",
            };
            require(
                package.ecosystem == ecosystem
                    && !metadata.name().trim().is_empty()
                    && package.version.as_deref() == metadata.version()
                    && metadata.source().repository == package.repository
                    && path == std::path::Path::new(metadata.source().path.as_str()),
                "package metadata identity",
            )?;
            links.source(metadata.source())?;
            if let PackageMetadata::Julia(metadata) = metadata {
                for field in metadata.fields.values() {
                    require(
                        field.source.repository == metadata.source.repository
                            && field.source.path == metadata.source.path,
                        "Julia metadata field source",
                    )?;
                    links.source(&field.source)?;
                }
            }
        }
        if let Some(PackageMetadata::R(metadata)) = &package.metadata {
            let description = &metadata.description;
            let root = package.path.as_ref().map_or("", DiagnosticPath::as_str);
            let metadata_path = std::path::Path::new(root).join(package.metadata_path.as_str());
            require(
                package.ecosystem == "r"
                    && package.version.as_ref() == Some(&description.version)
                    && description.source.repository == package.repository
                    && metadata_path == std::path::Path::new(description.source.path.as_str()),
                "R package metadata identity",
            )?;
            for (name, value) in [
                ("Package", description.name.as_str()),
                ("Version", description.version.as_str()),
                ("Title", description.title.as_str()),
            ] {
                require(
                    !value.is_empty()
                        && description
                            .fields
                            .get(name)
                            .is_some_and(|field| field.value == value),
                    "R package metadata fields",
                )?;
            }
            require(
                description.description.as_ref()
                    == description
                        .fields
                        .get("Description")
                        .map(|field| &field.value)
                    && description.license.as_ref()
                        == description.fields.get("License").map(|field| &field.value),
                "R package metadata fields",
            )?;
            for (spelling, targets) in &metadata.help_links {
                require(
                    !spelling.is_empty() && !spelling.chars().any(char::is_control),
                    "R help spelling",
                )?;
                require(
                    targets.windows(2).all(|pair| pair[0] < pair[1]),
                    "R help candidate order",
                )?;
                for target in targets {
                    require(
                        target
                            .package
                            .as_bytes()
                            .first()
                            .is_some_and(u8::is_ascii_alphabetic)
                            && target
                                .package
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
                            && !target.topic.is_empty()
                            && !target.topic.chars().any(char::is_control),
                        "R help target",
                    )?;
                    if let Some((package, _)) = spelling.split_once("::") {
                        require(package == target.package, "R help package qualifier")?;
                    }
                }
            }
            links.source(&description.source)?;
            for field in description.fields.values() {
                require(
                    field.source.repository == description.source.repository
                        && field.source.path == description.source.path
                        && field.source.span.is_some_and(|span| span.start <= span.end),
                    "R metadata field source",
                )?;
                links.source(&field.source)?;
            }
            for dependency in metadata.description.dependencies.values().flatten() {
                links.source(&dependency.source)?;
                for constraint in &dependency.constraints {
                    links.source(&constraint.source)?;
                }
            }
            if let Some(runtime) = &metadata.runtime {
                require(!runtime.r_version.is_empty(), "R metadata runtime version")?;
                require(workspace.provenance.iter().any(|record| {
                    matches!(&record.activity, ProvenanceActivity::PackageMetadata { package: id, r_version, .. }
                        if id == package_id && r_version == &runtime.r_version)
                }), "R metadata runtime evidence")?;
            }
        }
        for id in package.extraction_targets.keys() {
            require(!id.is_empty(), "empty target identity")?;
        }
        for (target_id, target) in &package.extraction_targets {
            if target.extractor == "rust" {
                require(package.ecosystem == "rust", "Rust extraction ecosystem")?;
                require(
                    workspace
                        .provenance
                        .iter()
                        .filter(|record| {
                            matches!(&record.activity,
                    ProvenanceActivity::RustDoctests { target, .. }
                    if target.package == *package_id && target.target == *target_id)
                        })
                        .count()
                        == 1,
                    "Rust target requires completed native doctests",
                )?;
                require(
                    workspace.provenance.iter().any(|record| {
                        matches!(&record.activity,
                    ProvenanceActivity::Extraction { target, mode: ExtractionMode::Compiler, .. }
                    if target.package == *package_id && target.target == *target_id)
                    }),
                    "Rust target requires compiler extraction evidence",
                )?;
            }
        }
        for (item_id, item) in &package.items {
            if let Some(ItemLanguageData::Rust(data)) = &item.language_data {
                require(package.ecosystem == "rust", "Rust item ecosystem")?;
                let mut owners = std::collections::BTreeSet::from([item_id.as_str()]);
                let mut owner = data.owner.as_ref();
                while let Some(reference) = owner {
                    require(
                        reference.package == *package_id && owners.insert(&reference.item),
                        "Rust member owner cycle or package",
                    )?;
                    links.item(reference)?;
                    let Some(ItemLanguageData::Rust(parent)) =
                        &package.items[&reference.item].language_data
                    else {
                        return Err(SnapshotError::Invalid("Rust member owner language"));
                    };
                    owner = parent.owner.as_ref();
                }
                if let Some(context) = &data.implementation {
                    let owner = data
                        .owner
                        .as_ref()
                        .ok_or(SnapshotError::Invalid("Rust implementation member owner"))?;
                    let Some(ItemLanguageData::Rust(parent)) =
                        &package.items[&owner.item].language_data
                    else {
                        return Err(SnapshotError::Invalid("Rust implementation member owner"));
                    };
                    require(
                        parent.implementations.iter().any(|implementation| {
                            implementation.identity == *context
                                && implementation.members.iter().any(|member| {
                                    member.package == *package_id && member.item == *item_id
                                })
                        }),
                        "Rust implementation member association",
                    )?;
                }
                if data.declaration == RustDeclarationKind::Implementation {
                    let owner = data.owner.as_ref().ok_or(SnapshotError::Invalid(
                        "Rust implementation documentation owner",
                    ))?;
                    let Some(ItemLanguageData::Rust(parent)) =
                        &package.items[&owner.item].language_data
                    else {
                        return Err(SnapshotError::Invalid(
                            "Rust implementation documentation owner",
                        ));
                    };
                    require(
                        parent.implementations.iter().any(|implementation| {
                            implementation
                                .documentation_item
                                .as_ref()
                                .is_some_and(|reference| {
                                    reference.package == *package_id && reference.item == *item_id
                                })
                        }),
                        "Rust implementation documentation association",
                    )?;
                }
                let mut implementation_documents = std::collections::BTreeSet::new();
                for implementation in &data.implementations {
                    if let Some(reference) = &implementation.documentation_item {
                        links.item(reference)?;
                        require(
                            reference.package == *package_id,
                            "Rust implementation documentation package",
                        )?;
                        let document = &package.items[&reference.item];
                        require(
                            matches!(&document.language_data, Some(ItemLanguageData::Rust(document_data))
                            if document_data.declaration == RustDeclarationKind::Implementation
                                && document_data.owner.as_ref().is_some_and(|owner| owner.package == *package_id && owner.item == *item_id))
                                && document.documentation.is_some()
                                && implementation_documents.insert(reference),
                            "Rust implementation documentation ownership",
                        )?;
                    }
                    if let Some(reference) = &implementation.trait_target {
                        links.item(reference)?;
                        let trait_item =
                            &workspace.packages[&reference.package].items[&reference.item];
                        require(
                            implementation
                                .trait_name
                                .as_ref()
                                .is_some_and(|name| !name.is_empty())
                                && matches!(&trait_item.language_data, Some(ItemLanguageData::Rust(trait_data))
                                if trait_data.declaration == RustDeclarationKind::Trait),
                            "Rust implementation trait target",
                        )?;
                    }
                    for member in &implementation.members {
                        links.item(member)?;
                        require(
                            member.package == *package_id
                                && matches!(&package.items[&member.item].language_data,
                                Some(ItemLanguageData::Rust(member_data))
                                if member_data.owner.as_ref().is_some_and(|owner| owner.package == *package_id && owner.item == *item_id)
                                    && member_data.implementation.as_ref() == Some(&implementation.identity)),
                            "Rust implementation ownership",
                        )?;
                    }
                }
            }
            if let Some(ItemLanguageData::R(data)) = &item.language_data
                && let Some(topic) = &data.help_topic
            {
                require(
                    !topic.name.trim().is_empty()
                        && !topic.name.chars().any(char::is_control)
                        && topic.members.len() > 1
                        && topic.members.windows(2).all(|pair| pair[0] < pair[1])
                        && topic.members.contains(&topic.primary)
                        && topic
                            .members
                            .iter()
                            .any(|member| member.package == *package_id && member.item == *item_id),
                    "R shared help topic coordinates",
                )?;
                for member in &topic.members {
                    links.item(member)?;
                    let sibling = &workspace.packages[&member.package].items[&member.item];
                    require(
                        member.package == *package_id
                            && matches!(&sibling.language_data, Some(ItemLanguageData::R(data)) if data.help_topic.as_ref() == Some(topic))
                            && item.documentation.is_some()
                            && sibling.documentation == item.documentation,
                        "R shared help topic membership",
                    )?;
                }
            }
            for signature in &item.signatures {
                links.signature(&signature.signature)?;
                links.evidence(&signature.sources)?;
            }
            for alias in &item.aliases {
                links.evidence(&alias.sources)?;
            }
            if let Some(data) = &item.language_data {
                links.language(data)?;
            }
            links.provenance(&item.provenance)?;
            if let Some(document) = &item.documentation {
                links.document(document, false)?;
            }
        }
    }
    for collection in workspace.content_collections.values() {
        if !collection.mount.is_empty() {
            route(&collection.mount)?;
        }
    }
    for relationship in &workspace.relationships {
        links.provenance(&relationship.provenance)?;
    }
    for page in workspace.pages.values() {
        let allow_api = matches!(&page.kind, PageKind::Authored { collection }
            if workspace.content_collections.get(collection).is_some_and(|collection| collection.format == crate::documents::AuthoredFormat::Documenter));
        links.document(&page.document, allow_api)?;
        for record in &page.document.provenance {
            if let ProvenanceActivity::RMarkdown { package, .. } = &record.activity {
                require(
                    matches!(&page.owner, ContentOwner::Package { package: owner } if owner == package),
                    "R Markdown package owner",
                )?;
            }
        }
    }
    for concept in workspace.concepts.values() {
        if let Some(document) = &concept.documentation {
            links.document(document, false)?;
        }
    }
    Ok(())
}

fn route(value: &str) -> Result {
    require(
        DiagnosticPath::try_from(value).is_ok()
            && !value
                .chars()
                .any(|c| c.is_control() || matches!(c, '?' | '#' | '%')),
        "presentation path",
    )
}

fn rust_doctest_outcome(example: &RustDoctestOutcome, host: &str) -> bool {
    let flag = |name: &str| match example.attributes.get(name).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let (Some(no_run), Some(compile_fail), Some(should_panic), Some(true)) = (
        flag("no_run"),
        flag("compile_fail"),
        flag("should_panic"),
        flag("rust"),
    ) else {
        return false;
    };
    if compile_fail && !no_run {
        return false;
    }
    let ignored = match example.attributes.get("ignore").map(String::as_str) {
        Some("All") => true,
        Some("None") => false,
        Some(value) => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(value) else {
                return false;
            };
            let Some(targets) = value.get("Some").and_then(serde_json::Value::as_array) else {
                return false;
            };
            if !targets
                .iter()
                .all(|target| target.as_str().is_some_and(|target| !target.is_empty()))
            {
                return false;
            }
            targets
                .iter()
                .any(|target| host.contains(target.as_str().expect("checked target string")))
        }
        None => return false,
    };
    if ignored {
        return example.outcome == RustDoctestStatus::ExplicitSkip
            && example
                .reason
                .as_ref()
                .is_some_and(|reason| !reason.is_empty())
            && example.output.is_empty();
    }
    let expected = if compile_fail || (should_panic && !no_run) {
        RustDoctestStatus::ExpectedError
    } else if no_run {
        RustDoctestStatus::CompileOnly
    } else {
        RustDoctestStatus::Executed
    };
    example.outcome == expected && example.reason.is_none()
}

struct Links<'a>(&'a Workspace);

impl Links<'_> {
    fn repository(&self, id: &str) -> Result {
        require(
            self.0.repositories.contains_key(id),
            "missing source repository",
        )
    }

    fn source(&self, source: &SourceLocation) -> Result {
        self.repository(&source.repository)
    }

    fn diagnostic_source(&self, source: &DiagnosticSource) -> Result {
        match source {
            DiagnosticSource::Repository { repository, .. } => self.repository(repository),
            DiagnosticSource::Configuration { .. } => Ok(()),
        }
    }

    fn item(&self, item: &ItemReference) -> Result {
        require(
            self.0
                .packages
                .get(&item.package)
                .is_some_and(|p| p.items.contains_key(&item.item)),
            "missing referenced item",
        )
    }

    fn items(&self, items: &[ItemReference]) -> Result {
        for item in items {
            self.item(item)?;
        }
        Ok(())
    }

    fn evidence(&self, sources: &[SourceEvidence]) -> Result {
        for evidence in sources {
            self.source(&evidence.source)?;
        }
        Ok(())
    }

    fn provenance(&self, records: &[Provenance]) -> Result {
        for record in records {
            if let Some(source) = &record.source {
                self.diagnostic_source(source)?;
            }
            match &record.activity {
                ProvenanceActivity::RustDoctests {
                    target,
                    policy,
                    package_name,
                    package_version,
                    compiler_version,
                    host,
                    inputs,
                    examples,
                    ..
                } => {
                    require(
                        self.0.packages.get(&target.package).is_some_and(|package| {
                            package.ecosystem == "rust"
                                && package.version.as_ref() == Some(package_version)
                                && package
                                    .extraction_targets
                                    .get(&target.target)
                                    .is_some_and(|target| target.extractor == "rust")
                        }) && policy == "rustdoc-doctest-v1"
                            && !package_name.is_empty()
                            && !compiler_version.is_empty()
                            && !host.is_empty()
                            && !inputs.is_empty()
                            && record.source.is_none()
                            && record.span.is_none(),
                        "Rust doctest provenance",
                    )?;
                    let mut names = std::collections::BTreeSet::new();
                    for example in examples {
                        require(
                            !example.name.is_empty()
                                && names.insert(&example.name)
                                && example.line > 0,
                            "Rust doctest identity",
                        )?;
                        require(rust_doctest_outcome(example, host), "Rust doctest outcome")?;
                        if let Some(source) = &example.source {
                            self.source(source)?;
                        }
                    }
                    for input in inputs {
                        self.source(&input.source)?;
                    }
                }
                ProvenanceActivity::ApiExamples {
                    package,
                    language,
                    policy,
                    runtime_version,
                    package_name,
                    package_version,
                    input,
                    examples,
                    ..
                } => {
                    require(
                        self.0.packages.get(package).is_some_and(|p| {
                            p.ecosystem == *language && p.version.as_ref() == Some(package_version)
                        }) && matches!(language.as_str(), "python" | "julia")
                            && *policy == format!("{language}-doctest-v1")
                            && !runtime_version.is_empty()
                            && !package_name.is_empty()
                            && !examples.is_empty()
                            && examples.iter().all(|e| {
                                e.span.start < e.span.end
                                    && !e.source.trim().is_empty()
                                    && match e.outcome.as_str() {
                                        "executed" | "expected-error" => e.reason.is_none(),
                                        "explicit-skip" => {
                                            language == "python"
                                                && e.reason.as_deref()
                                                    == Some("doctest +SKIP directive")
                                                && e.options
                                                    .get("SKIP")
                                                    .is_some_and(|v| v == "true")
                                                && e.actual.is_empty()
                                        }
                                        _ => false,
                                    }
                            }),
                        "API example execution provenance",
                    )?;
                    self.source(&input.source)?;
                }
                ProvenanceActivity::JuliaRuntime {
                    target,
                    policy,
                    julia_version,
                    package_name,
                    package_uuid,
                    package_version,
                    dependencies,
                } => {
                    require(
                        self.0.packages.get(&target.package).is_some_and(|p| {
                            p.extraction_targets
                                .get(&target.target)
                                .is_some_and(|t| t.extractor == "julia")
                        }) && policy == "julia-runtime-v1"
                            && !julia_version.is_empty()
                            && !package_name.is_empty()
                            && !package_uuid.is_empty()
                            && !package_version.is_empty()
                            && !dependencies.is_empty()
                            && record.source.is_none()
                            && record.span.is_none(),
                        "Julia runtime provenance",
                    )?;
                }
                ProvenanceActivity::RMarkdown {
                    package,
                    policy,
                    r_version,
                    knitr_version,
                    package_version,
                    dependencies,
                    input,
                    chunks,
                    ..
                } => {
                    require(
                        self.0.packages.contains_key(package)
                            && policy == "rmd-knitr-v1"
                            && !r_version.is_empty()
                            && !knitr_version.is_empty()
                            && !package_version.is_empty()
                            && !dependencies.is_empty()
                            && chunks.iter().all(|c| {
                                c.span.start <= c.span.end
                                    && !c.label.is_empty()
                                    && matches!(
                                        c.outcome.as_str(),
                                        "executed"
                                            | "illustrative"
                                            | "conditional-skip"
                                            | "allowed-error"
                                    )
                            }),
                        "R Markdown execution provenance",
                    )?;
                    self.source(&input.source)?;
                }
                ProvenanceActivity::Declaration => {}
                ProvenanceActivity::DecodedDocumentation { decoded_span } => {
                    require(
                        decoded_span.start <= decoded_span.end
                            && record.span.is_some_and(|span| span.start <= span.end)
                            && record.source.is_some(),
                        "decoded documentation provenance",
                    )?;
                }
                ProvenanceActivity::GeneratedMarkdown { collection, .. } => {
                    require(
                        self.0.content_collections.contains_key(collection),
                        "missing provenance collection",
                    )?;
                }
                ProvenanceActivity::Extraction { target, inputs, .. } => {
                    require(
                        self.0
                            .packages
                            .get(&target.package)
                            .is_some_and(|p| p.extraction_targets.contains_key(&target.target)),
                        "missing provenance target",
                    )?;
                    for repository in inputs.keys() {
                        self.repository(repository)?;
                    }
                }
                ProvenanceActivity::PackageMetadata {
                    package,
                    r_version,
                    policy,
                    inputs,
                } => {
                    require(
                        self.0.packages.contains_key(package)
                            && !r_version.is_empty()
                            && policy == "r-package-metadata-v1"
                            && !inputs.is_empty(),
                        "package metadata provenance",
                    )?;
                    for input in inputs {
                        self.source(&input.source)?;
                    }
                }
                ProvenanceActivity::RdExamples {
                    package,
                    policy,
                    r_version,
                    package_version,
                    dependencies,
                    input,
                    skipped,
                    ..
                } => {
                    require(
                        self.0.packages.contains_key(package)
                            && policy == "rd-examples-v1"
                            && !r_version.is_empty()
                            && !package_version.is_empty()
                            && !dependencies.is_empty()
                            && skipped.iter().all(|reason| !reason.is_empty()),
                        "Rd example provenance",
                    )?;
                    self.source(&input.source)?;
                }
                ProvenanceActivity::Execution {
                    declared_environment_inputs,
                    ..
                } => {
                    for input in declared_environment_inputs {
                        self.source(&input.source)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn optional_expression(&self, expression: &Option<SignatureExpression>) -> Result {
        if let Some(expression) = expression {
            self.expression(expression)?;
        }
        Ok(())
    }

    fn expressions(&self, expressions: &[SignatureExpression]) -> Result {
        for expression in expressions {
            self.expression(expression)?;
        }
        Ok(())
    }

    fn expression(&self, expression: &SignatureExpression) -> Result {
        match expression {
            SignatureExpression::Name { target, .. } => {
                if let Some(item) = target {
                    self.item(item)?;
                }
                Ok(())
            }
            SignatureExpression::Literal { .. } => Ok(()),
            SignatureExpression::Apply {
                constructor,
                arguments,
            } => {
                self.expression(constructor)?;
                self.expressions(arguments)
            }
            SignatureExpression::Sequence { items } => self.expressions(items),
            SignatureExpression::LanguageSpecific { children, .. } => self.expressions(children),
        }
    }

    fn signature(&self, signature: &Signature) -> Result {
        match signature {
            Signature::Callable {
                parameters,
                returns,
            } => {
                for parameter in parameters {
                    self.optional_expression(&parameter.annotation)?;
                    self.optional_expression(&parameter.default)?;
                }
                self.optional_expression(returns)
            }
            Signature::Value { annotation, value } => {
                self.optional_expression(annotation)?;
                self.optional_expression(value)
            }
            Signature::LanguageSpecific { syntax, .. } => self.expression(syntax),
        }
    }

    fn language(&self, data: &ItemLanguageData) -> Result {
        match data {
            ItemLanguageData::Rust(data) => {
                require(
                    !data.public_path.is_empty() && !data.public_path.chars().any(char::is_control),
                    "Rust public path",
                )?;
                if let Some(owner) = &data.owner {
                    self.item(owner)?;
                }
                let mut identities = std::collections::BTreeSet::new();
                for implementation in &data.implementations {
                    require(
                        !implementation.identity.is_empty()
                            && identities.insert(&implementation.identity),
                        "Rust implementation identity",
                    )?;
                    self.expression(&implementation.signature)?;
                    if let Some(item) = &implementation.documentation_item {
                        self.item(item)?;
                    }
                    if let Some(source) = &implementation.source_location {
                        self.source(source)?;
                    }
                    if let Some(target) = &implementation.trait_target {
                        self.item(target)?;
                    }
                    self.items(&implementation.members)?;
                }
                Ok(())
            }
            ItemLanguageData::Julia(data) => match &data.declaration {
                JuliaDeclaration::Callable { role, .. } => match role {
                    JuliaCallableRole::Family { methods } => self.items(methods),
                    JuliaCallableRole::Method { family, dispatch } => {
                        self.item(family)?;
                        self.expression(dispatch)
                    }
                },
                JuliaDeclaration::Type {
                    parameters,
                    supertype,
                    constructors,
                    ..
                } => {
                    self.expressions(parameters)?;
                    self.optional_expression(supertype)?;
                    self.items(constructors)
                }
                JuliaDeclaration::Module { .. }
                | JuliaDeclaration::Constant
                | JuliaDeclaration::Field { .. } => Ok(()),
            },
            ItemLanguageData::Python(data) => {
                for decorator in &data.decorators {
                    self.expression(&decorator.expression)?;
                    self.evidence(&decorator.sources)?;
                }
                match &data.declaration {
                    PythonDeclaration::Module { exports, .. } => match exports {
                        PythonExports::Implicit => Ok(()),
                        PythonExports::Explicit { sources, .. } => self.evidence(sources),
                        PythonExports::Dynamic {
                            expression,
                            sources,
                        } => {
                            self.expression(expression)?;
                            self.evidence(sources)
                        }
                    },
                    PythonDeclaration::Callable { role, .. } => match role {
                        PythonCallableRole::Family { overloads } => self.items(overloads),
                        PythonCallableRole::Overload { family } => self.item(family),
                    },
                    PythonDeclaration::Class { bases, constructor } => {
                        self.expressions(bases)?;
                        match constructor {
                            PythonConstructor::Unspecified => Ok(()),
                            PythonConstructor::Explicit { item } => self.item(item),
                            PythonConstructor::Dataclass { fields, .. } => self.items(fields),
                        }
                    }
                    PythonDeclaration::TypeAlias { target } => self.expression(target),
                    PythonDeclaration::Property { .. }
                    | PythonDeclaration::Constant
                    | PythonDeclaration::Field => Ok(()),
                }
            }
            ItemLanguageData::R(data) => match &data.declaration {
                RDeclaration::Constant
                | RDeclaration::DeprecatedTopic
                | RDeclaration::Dataset
                | RDeclaration::Function
                | RDeclaration::Constructor { .. } => Ok(()),
                RDeclaration::S3Generic {
                    dispatch_object,
                    methods,
                    ..
                } => {
                    self.optional_expression(dispatch_object)?;
                    self.items(methods)
                }
                RDeclaration::S3Method {
                    generic,
                    registration,
                    ..
                } => {
                    if let RGenericReference::Workspace { item } = generic {
                        self.item(item)?;
                    }
                    self.evidence(registration)
                }
            },
        }
    }

    fn document(&self, document: &SourcedDocument, allow_api: bool) -> Result {
        self.provenance(&document.provenance)?;
        if matches!(
            document.source_format,
            DocumentFormat::Authored {
                format: crate::documents::AuthoredFormat::Rmd
            }
        ) {
            let records: Vec<_> = document
                .provenance
                .iter()
                .filter_map(|p| match &p.activity {
                    ProvenanceActivity::RMarkdown {
                        input,
                        output,
                        package,
                        package_version,
                        dependencies,
                        chunks,
                        ..
                    } => Some((
                        input,
                        output,
                        package,
                        package_version,
                        dependencies,
                        chunks,
                    )),
                    _ => None,
                })
                .collect();
            require(
                records.len() == 1,
                "R Markdown requires completed native execution",
            )?;
            let (input, output, package, version, dependencies, chunks) = records[0];
            let Some(PackageMetadata::R(metadata)) = &self.0.packages[package].metadata else {
                return Err(SnapshotError::Invalid("R Markdown package metadata"));
            };
            let raw = document
                .raw_source
                .as_ref()
                .ok_or(SnapshotError::Invalid("R Markdown original source"))?;
            let generated = document
                .generated_source
                .as_ref()
                .ok_or(SnapshotError::Invalid("R Markdown evaluated source"))?;
            let (preparation, diagnostics) = crate::documents::prepare_rmd_document(raw);
            let inline = document
                .provenance
                .iter()
                .find_map(|p| match &p.activity {
                    ProvenanceActivity::RMarkdown { inline, .. } => Some(inline),
                    _ => None,
                })
                .expect("native evidence");
            require(
                preparation.inline == inline.iter().map(|item| item.span).collect::<Vec<_>>(),
                "R Markdown inline source association",
            )?;
            require(
                document.source_location.as_ref() == Some(&input.source)
                    && crate::provenance::fingerprint_bytes(raw.as_bytes()) == input.fingerprint
                    && crate::provenance::fingerprint_bytes(generated.as_bytes()) == *output
                    && crate::validation::r_versions_equal(version, &metadata.description.version)
                    && dependencies.contains_key("knitr")
                    && dependencies.get(&metadata.description.name) == Some(version)
                    && diagnostics.is_empty()
                    && preparation.chunks == chunks.iter().map(|c| c.span).collect::<Vec<_>>(),
                "R Markdown execution source association",
            )?;
        } else {
            require(
                document.generated_source.is_none(),
                "evaluated source belongs to R Markdown",
            )?;
        }
        for record in &document.provenance {
            if let ProvenanceActivity::ApiExamples {
                input,
                examples,
                language,
                ..
            } = &record.activity
            {
                let raw = document
                    .raw_source
                    .as_deref()
                    .ok_or(SnapshotError::Invalid("API example source"))?;
                require(
                    matches!(&document.source_format, DocumentFormat::Extracted { name }
                        if (language == "python" && name == "numpy-docstring")
                            || (language == "julia" && name == "julia-markdown"))
                        && document.source_location.as_ref() == Some(&input.source)
                        && crate::provenance::fingerprint_bytes(raw.as_bytes())
                            == input.fingerprint
                        && examples
                            .iter()
                            .all(|e| raw.get(e.span.start..e.span.end).is_some())
                        && examples
                            .windows(2)
                            .all(|pair| pair[0].span.end <= pair[1].span.start),
                    "API example source association",
                )?;
            }
            if let ProvenanceActivity::RdExamples {
                input,
                package,
                package_version,
                dependencies,
                ..
            } = &record.activity
            {
                let Some(PackageMetadata::R(metadata)) = &self.0.packages[package].metadata else {
                    return Err(SnapshotError::Invalid("Rd example package metadata"));
                };
                require(
                    matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "rd")
                        && document.source_location.as_ref() == Some(&input.source)
                        && document.raw_source.as_ref().is_some_and(|raw| {
                            crate::provenance::fingerprint_bytes(raw.as_bytes())
                                == input.fingerprint
                        })
                        && crate::validation::r_versions_equal(
                            package_version,
                            &metadata.description.version,
                        )
                        && dependencies.get(&metadata.description.name) == Some(package_version),
                    "Rd example source and package evidence",
                )?;
            }
        }
        for record in &document.provenance {
            if let ProvenanceActivity::DecodedDocumentation { decoded_span } = record.activity {
                require(
                    decoded_span.end <= document.document.span.end,
                    "decoded documentation range",
                )?;
            }
        }
        self.blocks(
            &document.document.blocks,
            allow_api
                && matches!(
                    document.source_format,
                    DocumentFormat::Authored {
                        format: crate::documents::AuthoredFormat::Documenter
                    }
                ),
        )
    }

    fn blocks(&self, blocks: &[Block], allow_api: bool) -> Result {
        for block in blocks {
            match block {
                Block::ApiDocumentation {
                    item, identifier, ..
                } => {
                    self.item(item)?;
                    require(
                        allow_api
                            && !identifier.is_empty()
                            && matches!(
                                self.0.packages[&item.package].items[&item.item].language_data,
                                Some(ItemLanguageData::Julia(_))
                            ),
                        "embedded Julia API item",
                    )?;
                }
                Block::BlockQuote { blocks, .. }
                | Block::Container { blocks, .. }
                | Block::Component { blocks, .. }
                | Block::Tabset { blocks, .. }
                | Block::Callout { blocks, .. } => self.blocks(blocks, allow_api)?,
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&item.blocks, allow_api)?;
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &row.cells {
                            self.blocks(&cell.blocks, allow_api)?;
                        }
                    }
                }
                Block::CodeCell(cell) => {
                    for option in &cell.resolved_options {
                        let indices = match &option.resolution {
                            CellOptionResolution::Resolved { declaration } => {
                                std::slice::from_ref(declaration)
                            }
                            CellOptionResolution::Ambiguous { declarations } => {
                                declarations.as_slice()
                            }
                        };
                        require(
                            !indices.is_empty()
                                && indices.iter().all(|&i| {
                                    cell.options.get(i).is_some_and(|d| {
                                        d.canonical_key.as_ref() == Some(&option.key)
                                    })
                                }),
                            "cell option declaration reference",
                        )?;
                    }
                    for output in &cell.outputs {
                        self.provenance(&output.provenance)?;
                        for representation in &output.representations {
                            if let OutputRepresentation::MarkdownBlocks { blocks, .. } =
                                representation
                            {
                                self.blocks(blocks, false)?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod rust_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn reference(item: &str) -> ItemReference {
        ItemReference {
            package: "sample".into(),
            item: item.into(),
        }
    }

    fn rust_item(name: &str, declaration: RustDeclarationKind, owner: Option<&str>) -> Item {
        Item {
            kind: ItemKind::Type,
            name: name.into(),
            qualified_name: format!("sample::{name}"),
            language_data: Some(ItemLanguageData::Rust(RustItemData {
                declaration,
                public_path: format!("sample::{name}"),
                owner: owner.map(reference),
                implementation: None,
                implementations: Vec::new(),
                attributes: Vec::new(),
            })),
            aliases: Vec::new(),
            signatures: Vec::new(),
            documentation: None,
            source_location: None,
            children: Vec::new(),
            provenance: Vec::new(),
        }
    }

    fn rust_data<'a>(workspace: &'a mut Workspace, item: &str) -> &'a mut RustItemData {
        let Some(ItemLanguageData::Rust(data)) = &mut workspace
            .packages
            .get_mut("sample")
            .unwrap()
            .items
            .get_mut(item)
            .unwrap()
            .language_data
        else {
            panic!("Rust test item")
        };
        data
    }

    fn workspace() -> Workspace {
        let mut type_item = rust_item("Type", RustDeclarationKind::Struct, None);
        let Some(ItemLanguageData::Rust(data)) = &mut type_item.language_data else {
            unreachable!()
        };
        data.implementations.push(RustImplementation {
            identity: "impl Trait for Type".into(),
            signature: SignatureExpression::Literal {
                text: "impl Trait for Type".into(),
            },
            documentation_item: Some(reference("docs")),
            source_location: None,
            trait_name: Some("Trait".into()),
            trait_target: Some(reference("trait")),
            members: vec![reference("method")],
            is_synthetic: false,
            is_blanket: false,
        });
        let mut method = rust_item("method", RustDeclarationKind::Method, Some("type"));
        let Some(ItemLanguageData::Rust(data)) = &mut method.language_data else {
            unreachable!()
        };
        data.implementation = Some("impl Trait for Type".into());
        let mut docs = rust_item(
            "implementation",
            RustDeclarationKind::Implementation,
            Some("type"),
        );
        docs.documentation = Some(SourcedDocument {
            document: Document {
                span: SourceSpan { start: 0, end: 0 },
                frontmatter: None,
                blocks: Vec::new(),
            },
            source_format: DocumentFormat::Extracted {
                name: "rustdoc".into(),
            },
            source_location: None,
            raw_source: None,
            generated_source: None,
            provenance: Vec::new(),
        });
        let package = Package {
            slug: "sample".into(),
            name: "Sample".into(),
            ecosystem: "rust".into(),
            version: Some("0.1.0".into()),
            metadata: None,
            repository: "repo".into(),
            path: None,
            metadata_path: "Cargo.toml".try_into().unwrap(),
            kind: PackageKind::Package,
            visibility: PackageVisibility::Public,
            extraction_targets: BTreeMap::new(),
            items: BTreeMap::from([
                ("type".into(), type_item),
                ("method".into(), method),
                ("docs".into(), docs),
                (
                    "trait".into(),
                    rust_item("Trait", RustDeclarationKind::Trait, None),
                ),
            ]),
        };
        Workspace {
            repositories: BTreeMap::from([(
                "repo".into(),
                Repository {
                    canonical_url: None,
                    source_link_template: None,
                    revision: None,
                    dirty: None,
                    declared_input_fingerprint: None,
                },
            )]),
            packages: BTreeMap::from([("sample".into(), package)]),
            ..Workspace::default()
        }
    }

    #[test]
    fn rejects_rust_implementation_documents_owned_by_another_type() {
        let valid = workspace();
        validate(&valid).unwrap();
        let mut wrong_owner = valid.clone();
        rust_data(&mut wrong_owner, "docs").owner = Some(reference("trait"));
        assert!(validate(&wrong_owner).is_err());

        let mut wrong_document = valid.clone();
        wrong_document
            .packages
            .get_mut("sample")
            .unwrap()
            .items
            .remove("docs");
        rust_data(&mut wrong_document, "type").implementations[0].documentation_item =
            Some(reference("method"));
        assert!(validate(&wrong_document).is_err());

        let mut orphan = valid.clone();
        rust_data(&mut orphan, "type").implementations[0].documentation_item = None;
        assert!(validate(&orphan).is_err());
    }

    #[test]
    fn rejects_missing_rust_implementation_and_member_references() {
        let valid = workspace();
        validate(&valid).unwrap();
        for missing in ["docs", "method", "trait", "type"] {
            let mut corrupted = valid.clone();
            corrupted
                .packages
                .get_mut("sample")
                .unwrap()
                .items
                .remove(missing);
            assert!(validate(&corrupted).is_err(), "accepted missing {missing}");
        }
        let mut missing_implementation = valid.clone();
        rust_data(&mut missing_implementation, "type")
            .implementations
            .clear();
        assert!(validate(&missing_implementation).is_err());
        let mut missing_context = valid.clone();
        rust_data(&mut missing_context, "method").implementation = None;
        assert!(validate(&missing_context).is_err());
        let mut wrong_trait = valid.clone();
        rust_data(&mut wrong_trait, "type").implementations[0].trait_target =
            Some(reference("type"));
        assert!(validate(&wrong_trait).is_err());
    }

    #[test]
    fn rejects_cross_package_rust_owners_and_implementation_documents() {
        let mut valid = workspace();
        let mut foreign = valid.packages["sample"].clone();
        foreign.slug = "foreign".into();
        foreign.items.clear();
        let mut item = rust_item("foreign", RustDeclarationKind::Struct, None);
        item.language_data = None;
        foreign.items.insert("existing".into(), item);
        valid.packages.insert("foreign".into(), foreign);
        validate(&valid).unwrap();
        let foreign = ItemReference {
            package: "foreign".into(),
            item: "existing".into(),
        };
        let mut wrong_owner = valid.clone();
        rust_data(&mut wrong_owner, "method").owner = Some(foreign.clone());
        assert!(validate(&wrong_owner).is_err());
        let mut wrong_document = valid;
        wrong_document
            .packages
            .get_mut("sample")
            .unwrap()
            .items
            .remove("docs");
        rust_data(&mut wrong_document, "type").implementations[0].documentation_item =
            Some(foreign);
        assert!(validate(&wrong_document).is_err());
    }

    fn example(
        outcome: RustDoctestStatus,
        no_run: bool,
        compile_fail: bool,
        should_panic: bool,
        ignore: &str,
    ) -> RustDoctestOutcome {
        RustDoctestOutcome {
            name: "src/lib.rs - (line 1)".into(),
            source: None,
            line: 1,
            code: "assert!(true);".into(),
            attributes: BTreeMap::from([
                ("no_run".into(), no_run.to_string()),
                ("compile_fail".into(), compile_fail.to_string()),
                ("should_panic".into(), should_panic.to_string()),
                ("rust".into(), "true".into()),
                ("ignore".into(), ignore.into()),
            ]),
            outcome,
            output: String::new(),
            reason: (outcome == RustDoctestStatus::ExplicitSkip)
                .then(|| "Native ignore attribute.".into()),
        }
    }

    #[test]
    fn rust_doctest_statuses_require_their_native_semantic_flags() {
        let host = "x86_64-unknown-linux-gnu";
        for valid in [
            example(RustDoctestStatus::Executed, false, false, false, "None"),
            example(RustDoctestStatus::CompileOnly, true, false, false, "None"),
            example(RustDoctestStatus::CompileOnly, true, false, true, "None"),
            example(RustDoctestStatus::ExpectedError, true, true, false, "None"),
            example(RustDoctestStatus::ExpectedError, false, false, true, "None"),
            example(RustDoctestStatus::ExplicitSkip, false, false, false, "All"),
            example(
                RustDoctestStatus::ExplicitSkip,
                false,
                false,
                false,
                r#"{"Some":["linux"]}"#,
            ),
            example(
                RustDoctestStatus::Executed,
                false,
                false,
                false,
                r#"{"Some":["windows"]}"#,
            ),
        ] {
            assert!(rust_doctest_outcome(&valid, host), "{valid:?}");
        }
        for invalid in [
            example(RustDoctestStatus::Executed, true, false, false, "None"),
            example(RustDoctestStatus::Executed, false, false, true, "None"),
            example(RustDoctestStatus::Executed, false, false, false, "All"),
            example(RustDoctestStatus::CompileOnly, true, true, false, "None"),
            example(
                RustDoctestStatus::ExpectedError,
                false,
                false,
                false,
                "None",
            ),
            example(RustDoctestStatus::ExpectedError, false, true, false, "None"),
            example(RustDoctestStatus::ExplicitSkip, false, false, false, "None"),
            example(
                RustDoctestStatus::ExplicitSkip,
                false,
                false,
                false,
                r#"{"Some":["windows"]}"#,
            ),
        ] {
            assert!(!rust_doctest_outcome(&invalid, host), "{invalid:?}");
        }
    }
}

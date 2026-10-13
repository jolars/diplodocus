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
        for (item_id, item) in &package.items {
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
                links.document(document)?;
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
        links.document(&page.document)?;
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
            links.document(document)?;
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
                RDeclaration::Dataset
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

    fn document(&self, document: &SourcedDocument) -> Result {
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
        self.blocks(&document.document.blocks)
    }

    fn blocks(&self, blocks: &[Block]) -> Result {
        for block in blocks {
            match block {
                Block::BlockQuote { blocks, .. }
                | Block::Container { blocks, .. }
                | Block::Callout { blocks, .. } => self.blocks(blocks)?,
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&item.blocks)?;
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &row.cells {
                            self.blocks(&cell.blocks)?;
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
                                self.blocks(blocks)?;
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

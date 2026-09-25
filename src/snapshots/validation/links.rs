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
    for package in workspace.packages.values() {
        route(&package.slug)?;
        for id in package.extraction_targets.keys() {
            require(!id.is_empty(), "empty target identity")?;
        }
        for item in package.items.values() {
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
                ProvenanceActivity::Declaration => {}
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
                RDeclaration::Function | RDeclaration::Constructor { .. } => Ok(()),
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
        self.blocks(&document.document.blocks)
    }

    fn blocks(&self, blocks: &[Block]) -> Result {
        for block in blocks {
            match block {
                Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                    self.blocks(blocks)?
                }
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

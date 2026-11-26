//! Source selection and presentation shared by execution and snapshot validation.

use super::{AstNode, CodeBlock, Flavor, ParserOptions, span};
use crate::configuration::{ExecutionEngine, ExecutionMode};
use crate::ir::*;

pub(crate) struct Example {
    pub span: SourceSpan,
    pub language: String,
    pub package: String,
    pub code: String,
}

pub(crate) fn enabled(collection: &ContentCollection) -> bool {
    collection.format == super::AuthoredFormat::Gfm
        && collection.execution.mode == ExecutionMode::Execute
        && collection.execution.engine == Some(ExecutionEngine::Native)
}

pub(crate) fn plan(
    source: &str,
    collection: &ContentCollection,
    workspace: &Workspace,
    available: impl Fn(&str) -> bool,
) -> Result<Vec<Example>, (SourceSpan, String)> {
    if !enabled(collection) {
        return Ok(Vec::new());
    }

    let mut options = ParserOptions::for_flavor(Flavor::Gfm);
    options.extensions.yaml_metadata_block = true;
    options.extensions.fenced_divs = true;
    options.extensions.header_attributes = true;
    let parsed = panache_parser::parse_document(source, Some(options));
    let mut examples = Vec::new();
    for code in parsed
        .document()
        .syntax()
        .descendants()
        .filter_map(CodeBlock::cast)
    {
        let Some(info) = code.info() else { continue };
        let info = info.syntax().text().to_string();
        let mut words = info.split_whitespace();
        let Some(language) = words.next() else {
            continue;
        };
        if words.next() != Some("run") {
            continue;
        }
        let location = span(code.syntax().text_range());
        let error = |message| (location, message);
        let selector = match words.next() {
            None => None,
            Some(word) => Some(
                word.strip_prefix("package=")
                    .filter(|id| !id.is_empty() && words.next().is_none())
                    .ok_or_else(|| error("Expected optionally one package=ID after run.".into()))?,
            ),
        };
        let language = language.to_ascii_lowercase();
        if !matches!(language.as_str(), "python" | "r") {
            return Err(error(format!(
                "Native guide execution does not support {language}."
            )));
        }
        let eligible = |id: &str, package: &Package| {
            available(id)
                && package.ecosystem == language
                && package
                    .extraction_targets
                    .values()
                    .any(|target| target.extractor == language)
        };
        let owner = match &collection.owner {
            ContentOwner::Project => None,
            ContentOwner::Package { package } => Some(package.as_str()),
        };
        let package = if let Some(id) = selector.or(owner) {
            workspace
                .packages
                .get(id)
                .filter(|package| eligible(id, package))
                .ok_or_else(|| {
                    error(format!(
                        "Package {id} needs a compatible native {language} extraction target."
                    ))
                })?;
            id.to_owned()
        } else {
            let mut matches = workspace
                .packages
                .iter()
                .filter(|(id, package)| eligible(id, package));
            let Some((id, _)) = matches.next() else {
                return Err(error(format!(
                    "No package has a native {language} extraction target."
                )));
            };
            if matches.next().is_some() {
                return Err(error(format!(
                    "Multiple {language} packages match. Select one with package=ID."
                )));
            }
            id.clone()
        };
        examples.push(Example {
            span: location,
            language,
            package,
            code: code.code_source(),
        });
    }
    Ok(examples)
}

pub(crate) fn project(blocks: &mut [Block], results: &[GuideExampleOutcome]) -> bool {
    let mut matched = 0;
    super::documenter::rewrite(blocks, &mut |block| {
        let Block::CodeBlock { span, .. } = block else {
            return;
        };
        let Some(result) = results.get(matched).filter(|result| result.span == *span) else {
            return;
        };
        matched += 1;
        if result.outputs.is_empty() {
            return;
        }
        let span = *span;
        let output = result
            .outputs
            .iter()
            .map(|output| super::documenter::rendered_output(output, span))
            .collect();
        *block = Block::Container {
            kind: DocumentContainerKind::GeneratedChunk,
            identifier: None,
            blocks: vec![
                block.clone(),
                Block::Container {
                    kind: DocumentContainerKind::GeneratedOutput,
                    identifier: None,
                    blocks: output,
                    span,
                },
            ],
            span,
        };
    });
    matched == results.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(ecosystem: &str) -> Package {
        serde_json::from_value(serde_json::json!({
            "slug": ecosystem, "name": ecosystem, "ecosystem": ecosystem,
            "version": null, "repository": "repo", "path": null,
            "metadata_path": "metadata", "kind": "package", "visibility": "public",
            "extraction_targets": {"api": {"extractor": ecosystem, "path": null, "role": "public-api"}},
            "items": {}
        })).unwrap()
    }

    #[test]
    fn runnable_fences_resolve_only_in_enabled_collections() {
        let mut collection = ContentCollection {
            owner: ContentOwner::Project,
            repository: "repo".into(),
            path: None,
            mount: String::new(),
            format: super::super::AuthoredFormat::Gfm,
            navigation: Default::default(),
            execution: Default::default(),
        };
        let mut workspace = Workspace::default();
        let implicit = "```python run\nprint(1)\n```\n";
        assert!(
            plan(implicit, &collection, &workspace, |_| true)
                .unwrap()
                .is_empty()
        );
        collection.execution.mode = ExecutionMode::Execute;
        collection.execution.engine = Some(ExecutionEngine::Native);
        assert!(plan(implicit, &collection, &workspace, |_| true).is_err());
        workspace.packages.insert("first".into(), package("python"));
        let source = format!("```r\nstop()\n```\n\n{implicit}");
        let planned = plan(&source, &collection, &workspace, |_| true).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].package, "first");
        assert_eq!(planned[0].code, "print(1)\n");

        workspace
            .packages
            .insert("second".into(), package("python"));
        assert!(plan(implicit, &collection, &workspace, |_| true).is_err());
        collection.owner = ContentOwner::Package {
            package: "first".into(),
        };
        assert_eq!(
            plan(implicit, &collection, &workspace, |_| true).unwrap()[0].package,
            "first"
        );
        let explicit = "```python run package=second\npass\n```\n";
        assert_eq!(
            plan(explicit, &collection, &workspace, |_| true).unwrap()[0].package,
            "second"
        );
        assert!(
            plan(
                &explicit.replace("python", "r"),
                &collection,
                &workspace,
                |_| true
            )
            .is_err()
        );
        assert!(
            plan(
                &explicit.replace("second", "missing"),
                &collection,
                &workspace,
                |_| true
            )
            .is_err()
        );
        assert!(
            plan(
                &explicit.replace("second", "second package=first"),
                &collection,
                &workspace,
                |_| true
            )
            .is_err()
        );
    }
}

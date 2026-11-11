//! Whole-file include expansion with original source evidence.

use panache_parser::syntax::BlockNode;
use panache_parser::{Flavor, ParserOptions};

use super::*;
use crate::documents::AuthoredFormat;
use crate::ir::SourceSpan;

/// One contiguous part of expanded Markdown and the source bytes it came from.
#[derive(Debug, Clone)]
pub struct IncludeSourceRange {
    /// Byte range in the expanded page.
    pub expanded: SourceSpan,
    /// Original repository file and byte range.
    pub original: SourceLocation,
    /// One-based line number at the start of the original range.
    pub line: usize,
    /// Directives traversed from the main page to this source.
    pub include_chain: Vec<SourceLocation>,
}

pub(super) struct Expanded {
    pub text: String,
    pub ranges: Vec<IncludeSourceRange>,
    pub inputs: Vec<(SourceLocation, String)>,
}

pub(super) fn expand(
    text: &str,
    path: &Path,
    root: &Path,
    repository: &crate::paths::ResolvedRepositoryPaths,
    format: AuthoredFormat,
) -> Result<Expanded, AssemblyError> {
    let mut expanded = Expanded {
        text: String::new(),
        ranges: Vec::new(),
        inputs: Vec::new(),
    };
    if !matches!(format, AuthoredFormat::Gfm | AuthoredFormat::Qmd) {
        expanded.text = text.to_owned();
        return Ok(expanded);
    }
    let mut context = Context {
        root,
        repository,
        format,
        active: Vec::new(),
        chain: Vec::new(),
        expanded: &mut expanded,
    };
    context.file(path, text)?;
    Ok(expanded)
}

struct Context<'a> {
    root: &'a Path,
    repository: &'a crate::paths::ResolvedRepositoryPaths,
    format: AuthoredFormat,
    active: Vec<PathBuf>,
    chain: Vec<SourceLocation>,
    expanded: &'a mut Expanded,
}

impl Context<'_> {
    fn error(&self, location: &SourceLocation, message: &str) -> AssemblyError {
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::InvalidMarkdownInclude,
            Severity::Error,
            message,
        );
        diagnostic.source = Some(source(location));
        diagnostic.span = location.span;
        append_chain(&mut diagnostic, &self.chain);
        AssemblyError::Diagnostics(vec![diagnostic])
    }

    fn file(&mut self, path: &Path, text: &str) -> Result<(), AssemblyError> {
        let location = self.repository.source_location(path, None)?;
        let canonical = self.repository.path.join(location.path.as_str());
        self.active.push(canonical.clone());
        self.expanded
            .inputs
            .push((location.clone(), text.to_owned()));
        let flavor = if self.format == AuthoredFormat::Gfm {
            Flavor::Gfm
        } else {
            Flavor::Quarto
        };
        let mut options = ParserOptions::for_flavor(flavor);
        options.extensions.yaml_metadata_block = true;
        let parsed = panache_parser::parse_document(text, Some(options));
        if !self.chain.is_empty() && parsed.document().frontmatter().is_some() {
            return Err(self.error(
                self.chain.last().unwrap(),
                "Included files cannot declare frontmatter.",
            ));
        }
        let prefix = if self.format == AuthoredFormat::Gfm {
            "<!--@include"
        } else {
            "{{< include"
        };
        let mut cursor = 0;
        for block in parsed.document().block_nodes() {
            if matches!(block, BlockNode::CodeBlock(_)) {
                continue;
            }
            let range = block.text_range();
            let start = usize::from(range.start());
            let block_end = usize::from(range.end());
            // Keep the directive's line ending so a partial without a final
            // newline cannot merge with the following paragraph.
            let end = start + text[start..block_end].trim_end_matches(['\r', '\n']).len();
            let raw = &text[start..end];
            if !raw.starts_with(prefix) || (start > 0 && !text[..start].ends_with('\n')) {
                continue;
            }
            let mut directive = location.clone();
            directive.span = Some(SourceSpan { start, end });
            let before = text[..start].strip_suffix('\n').unwrap_or(&text[..start]);
            let after = &text[end..];
            let after = if raw.ends_with('\n') {
                after
            } else {
                after
                    .strip_prefix("\r\n")
                    .or_else(|| after.strip_prefix('\n'))
                    .unwrap_or(after)
            };
            if (start > 0 && !before.rsplit('\n').next().unwrap_or("").trim().is_empty())
                || !after.lines().next().unwrap_or("").trim().is_empty()
            {
                return Err(self.error(
                    &directive,
                    "Include directives require surrounding blank lines.",
                ));
            }
            let target = target(raw, self.format)
                .ok_or_else(|| self.error(&directive, "Invalid whole-file include directive."))?;
            let rooted = if self.format == AuthoredFormat::Gfm {
                target.strip_prefix("@/")
            } else {
                target.strip_prefix('/')
            };
            let requested = match rooted {
                Some(relative) => self.root.join(relative),
                None => canonical.parent().unwrap().join(&target),
            };
            let included = self
                .repository
                .source_location(&requested, None)
                .map_err(|_| {
                    self.error(
                        &directive,
                        "Include target must be a regular file inside the declared repository.",
                    )
                })?;
            let included_path = self.repository.path.join(included.path.as_str());
            let extension = included_path.extension().and_then(|s| s.to_str());
            if crate::generated_storage::is_reserved(&included_path)
                || !(extension == Some("md")
                    || (self.format == AuthoredFormat::Qmd && extension == Some("qmd")))
            {
                return Err(self.error(
                    &directive,
                    "Include target must be a Markdown file outside generated storage.",
                ));
            }
            if self.active.contains(&included_path) {
                return Err(self.error(&directive, "Markdown include cycle detected."));
            }
            // Bound recursion independently of cycle detection for deeply nested inputs.
            if self.active.len() >= 128 {
                return Err(self.error(&directive, "Markdown include nesting exceeds 128 files."));
            }
            let content = fs::read_to_string(&included_path)
                .map_err(|_| self.error(&directive, "Include target must be readable UTF-8."))?;
            self.copy(text, cursor, start, &location);
            self.chain.push(directive);
            self.file(&included_path, &content)?;
            self.chain.pop();
            cursor = end;
        }
        self.copy(text, cursor, text.len(), &location);
        self.active.pop();
        Ok(())
    }

    fn copy(&mut self, text: &str, start: usize, end: usize, location: &SourceLocation) {
        let output_start = self.expanded.text.len();
        self.expanded.text.push_str(&text[start..end]);
        let mut original = location.clone();
        original.span = Some(SourceSpan { start, end });
        self.expanded.ranges.push(IncludeSourceRange {
            expanded: SourceSpan {
                start: output_start,
                end: self.expanded.text.len(),
            },
            original,
            line: text[..start].bytes().filter(|b| *b == b'\n').count() + 1,
            include_chain: self.chain.clone(),
        });
    }
}

fn target(raw: &str, format: AuthoredFormat) -> Option<String> {
    let raw = raw.trim_end();
    let inner = if format == AuthoredFormat::Gfm {
        raw.strip_prefix("<!--@include:")?.strip_suffix("-->")?
    } else {
        raw.strip_prefix("{{< include")?.strip_suffix(">}}")?
    }
    .trim();
    let path = if inner.starts_with('"') {
        inner.strip_prefix('"')?.strip_suffix('"')?
    } else {
        if inner.chars().any(char::is_whitespace) {
            return None;
        }
        inner
    };
    if path.is_empty()
        || path.contains(['\n', '\r', '"', '?', '#', '{', '}', '\\', ':'])
        || path.starts_with("//")
        || (format == AuthoredFormat::Gfm && path.starts_with('/'))
    {
        return None;
    }
    Some(path.to_owned())
}

fn append_chain(diagnostic: &mut Diagnostic, chain: &[SourceLocation]) {
    for entry in chain {
        let span = entry.span.unwrap();
        diagnostic.message.push_str(&format!(
            " Included from {} at bytes {}..{}.",
            entry.path.as_str(),
            span.start,
            span.end
        ));
    }
}

pub(super) fn remap(diagnostic: &mut Diagnostic, ranges: &[IncludeSourceRange]) {
    let Some(span) = diagnostic.span else {
        return;
    };
    let Some(range) = ranges
        .iter()
        .find(|r| r.expanded.start <= span.start && span.start < r.expanded.end)
        .or_else(|| {
            ranges
                .iter()
                .rev()
                .find(|r| r.expanded.end == span.start && r.expanded.start < r.expanded.end)
        })
    else {
        return;
    };
    let original = range.original.span.unwrap();
    let translate = |s: SourceSpan| SourceSpan {
        start: original.start + s.start - range.expanded.start,
        end: original.start + s.end.min(range.expanded.end) - range.expanded.start,
    };
    diagnostic.span = Some(translate(span));
    // Related offsets belong to the same file only when they share this range.
    diagnostic
        .related_spans
        .retain(|s| s.start >= range.expanded.start && s.end <= range.expanded.end);
    for span in &mut diagnostic.related_spans {
        *span = translate(*span);
    }
    diagnostic.source = Some(source(&range.original));
    append_chain(diagnostic, &range.include_chain);
}

//! Markdown and verbatim code include expansion with original source evidence.

use panache_parser::syntax::{AstNode, BlockNode, CodeBlock, SyntaxKind};
use panache_parser::{Flavor, ParserOptions};

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSource, Severity};
use crate::documents::AuthoredFormat;
use crate::ir::{IncludeSourceRange, SourceLocation, SourceSpan};

#[derive(Debug, thiserror::Error)]
pub(crate) enum IncludeError {
    #[error(transparent)]
    Path(#[from] crate::paths::PathResolutionError),
    #[error("Markdown include expansion failed")]
    Diagnostics(Vec<Diagnostic>),
}

fn source(location: &SourceLocation) -> DiagnosticSource {
    DiagnosticSource::Repository {
        repository: location.repository.clone(),
        path: location.path.clone(),
    }
}

pub(crate) struct Expanded {
    pub text: String,
    pub ranges: Vec<IncludeSourceRange>,
    pub inputs: Vec<(SourceLocation, String)>,
}

pub(crate) fn expand(
    text: &str,
    path: &Path,
    root: &Path,
    repository: &crate::paths::ResolvedRepositoryPaths,
    format: AuthoredFormat,
) -> Result<Expanded, IncludeError> {
    expand_with_inputs(text, path, root, repository, format, &mut |_| {})
}

/// Retain attempted paths even when a missing nested target prevents expansion.
pub(crate) fn watch_inputs(
    text: &str,
    path: &Path,
    root: &Path,
    repository: &crate::paths::ResolvedRepositoryPaths,
    format: AuthoredFormat,
) -> Vec<PathBuf> {
    let mut inputs = Vec::new();
    let _ = expand_with_inputs(text, path, root, repository, format, &mut |input| {
        inputs.push(input.to_owned());
    });
    inputs
}

fn expand_with_inputs(
    text: &str,
    path: &Path,
    root: &Path,
    repository: &crate::paths::ResolvedRepositoryPaths,
    format: AuthoredFormat,
    on_input: &mut dyn FnMut(&Path),
) -> Result<Expanded, IncludeError> {
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
        on_input,
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
    on_input: &'a mut dyn FnMut(&Path),
}

impl Context<'_> {
    fn error(&self, location: &SourceLocation, message: &str) -> IncludeError {
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::InvalidMarkdownInclude,
            Severity::Error,
            message,
        );
        diagnostic.source = Some(source(location));
        diagnostic.span = location.span;
        append_chain(&mut diagnostic, &self.chain);
        IncludeError::Diagnostics(vec![diagnostic])
    }

    fn file(&mut self, path: &Path, text: &str) -> Result<(), IncludeError> {
        (self.on_input)(path);
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
        let (code_prefix, _) = code_delimiters(self.format);
        let mut cursor = 0;
        for block in parsed.document().block_nodes() {
            if let BlockNode::CodeBlock(code) = block {
                self.code_fence(code, text, &location, &mut cursor)?;
                continue;
            }
            let range = block.text_range();
            let start = usize::from(range.start());
            let block_end = usize::from(range.end());
            // Keep the directive's line ending so a partial without a final
            // newline cannot merge with the following paragraph.
            let end = start + text[start..block_end].trim_end_matches(['\r', '\n']).len();
            let raw = &text[start..end];
            if !raw.starts_with(prefix)
                || raw.starts_with(code_prefix)
                || (start > 0 && !text[..start].ends_with('\n'))
            {
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
            let (_, included_path) = self.resolve(&directive, target)?;
            let extension = included_path.extension().and_then(|s| s.to_str());
            if !(extension == Some("md")
                || (self.format == AuthoredFormat::Qmd && extension == Some("qmd")))
            {
                return Err(self.error(&directive, "Include target must be a Markdown file."));
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

    fn resolve(
        &mut self,
        directive: &SourceLocation,
        target: &str,
    ) -> Result<(SourceLocation, PathBuf), IncludeError> {
        let rooted = if self.format == AuthoredFormat::Gfm {
            target.strip_prefix("@/")
        } else {
            target.strip_prefix('/')
        };
        let requested = match rooted {
            Some(relative) => self.root.join(relative),
            None => self
                .repository
                .path
                .join(directive.path.as_str())
                .parent()
                .unwrap()
                .join(target),
        };
        (self.on_input)(&requested);
        let included = self
            .repository
            .source_location(&requested, None)
            .map_err(|_| {
                self.error(
                    directive,
                    "Include target must be a regular file inside the declared repository.",
                )
            })?;
        let path = self.repository.path.join(included.path.as_str());
        if crate::generated_storage::is_reserved(&path) {
            return Err(self.error(
                directive,
                "Include target must be outside generated storage.",
            ));
        }
        Ok((included, path))
    }

    fn code_fence(
        &mut self,
        code: CodeBlock,
        text: &str,
        location: &SourceLocation,
        cursor: &mut usize,
    ) -> Result<(), IncludeError> {
        let Some(open) = fence_marker(&code, SyntaxKind::CODE_FENCE_OPEN) else {
            return Ok(());
        };
        let Some(body) = code.content_range() else {
            return Ok(());
        };
        let close = fence_marker(&code, SyntaxKind::CODE_FENCE_CLOSE);
        let option_boundary = code.is_executable_chunk().then(|| {
            code.hashpipe_yaml_preamble()
                .map_or(body.start(), |preamble| {
                    preamble.syntax().text_range().end()
                })
        });
        let marker = text.as_bytes()[open.start];
        let mut width = open.end - open.start;
        let mut offset = usize::from(body.start());
        let (prefix, suffix) = code_delimiters(self.format);
        let mut snippets = Vec::new();
        for line in text[offset..usize::from(body.end())].split_inclusive('\n') {
            let start = offset;
            offset += line.len();
            let raw = line.trim_start_matches([' ', '\t']);
            let Some(inner) = raw.strip_prefix(prefix) else {
                continue;
            };
            let end = start + line.trim_end_matches(['\r', '\n']).len();
            let mut directive = location.clone();
            directive.span = Some(SourceSpan { start, end });
            let (target, selection) = inner
                .trim_end()
                .strip_suffix(suffix)
                .and_then(|inner| code_target(inner, self.format))
                .ok_or_else(|| {
                    self.error(
                        &directive,
                        "Invalid code include directive. Expected a path and optional {first-last} line range.",
                    )
                })?;
            let (origin, path) = self.resolve(&directive, target)?;
            if self.active.contains(&path) {
                return Err(self.error(
                    &directive,
                    "Code include target is already in the include chain.",
                ));
            }
            if self.active.len() >= 128 {
                return Err(self.error(&directive, "Markdown include nesting exceeds 128 files."));
            }
            let content = fs::read_to_string(path)
                .map_err(|_| self.error(&directive, "Include target must be readable UTF-8."))?;
            let selected = select_lines(&content, selection).ok_or_else(|| {
                self.error(
                    &directive,
                    "Code include line range exceeds the target file.",
                )
            })?;
            let snippet = &content[selected.start..selected.end];
            for line in snippet.split_inclusive('\n') {
                width = width.max(marker_run(line, marker) + 1);
            }
            // Keep a blank line before an include that could extend the cell's
            // YAML options. Reuse the directive's newline as the separator.
            let option_markers = ["#|", "//|", "--|", "%%|"];
            let separate_options = option_boundary.is_some_and(|boundary| {
                text.get(usize::from(boundary)..start)
                    .is_some_and(|before| {
                        before.is_empty() || option_markers.contains(&before.trim())
                    })
            }) && (snippet.is_empty()
                || option_markers
                    .iter()
                    .any(|prefix| snippet.trim_start_matches([' ', '\t']).starts_with(prefix)));
            snippets.push(CodeInclude {
                directive,
                origin,
                content,
                selected,
                indent_end: start + line.len() - raw.len(),
                line_end: offset,
                separate_options,
            });
        }
        if snippets.is_empty() {
            return Ok(());
        }

        self.widen(text, cursor, open, width, location);
        for snippet in snippets {
            let SourceSpan { start, end } = snippet.directive.span.unwrap();
            self.copy(text, *cursor, start, location);
            if snippet.separate_options {
                self.copy(text, end, snippet.line_end, location);
            }
            let mut chain = self.chain.clone();
            chain.push(snippet.directive);
            if start == snippet.indent_end || snippet.selected.start == snippet.selected.end {
                append_range(
                    self.expanded,
                    &snippet.content,
                    snippet.selected,
                    &snippet.origin,
                    line_number(&snippet.content, snippet.selected.start),
                    &chain,
                );
            } else {
                let mut source_offset = snippet.selected.start;
                let first_line = line_number(&snippet.content, source_offset);
                let indent_line = line_number(text, start);
                for (index, line) in snippet.content[source_offset..snippet.selected.end]
                    .split_inclusive('\n')
                    .enumerate()
                {
                    append_range(
                        self.expanded,
                        text,
                        SourceSpan {
                            start,
                            end: snippet.indent_end,
                        },
                        location,
                        indent_line,
                        &self.chain,
                    );
                    append_range(
                        self.expanded,
                        &snippet.content,
                        SourceSpan {
                            start: source_offset,
                            end: source_offset + line.len(),
                        },
                        &snippet.origin,
                        first_line + index,
                        &chain,
                    );
                    source_offset += line.len();
                }
            }
            if snippet.selected.start != snippet.selected.end
                && !snippet.content[..snippet.selected.end].ends_with('\n')
            {
                self.copy(text, end, snippet.line_end, location);
            }
            self.expanded.inputs.push((snippet.origin, snippet.content));
            *cursor = snippet.line_end;
        }
        if let Some(close) = close {
            self.widen(text, cursor, close, width, location);
        }
        Ok(())
    }

    fn widen(
        &mut self,
        text: &str,
        cursor: &mut usize,
        marker: SourceSpan,
        width: usize,
        location: &SourceLocation,
    ) {
        let marker_width = marker.end - marker.start;
        if width <= marker_width {
            return;
        }
        self.copy(text, *cursor, marker.end, location);
        // Every added marker byte maps to an identical byte in the authored fence.
        let line = line_number(text, marker.start);
        let mut remaining = width - marker_width;
        while remaining > 0 {
            let count = remaining.min(marker_width);
            append_range(
                self.expanded,
                text,
                SourceSpan {
                    start: marker.start,
                    end: marker.start + count,
                },
                location,
                line,
                &self.chain,
            );
            remaining -= count;
        }
        *cursor = marker.end;
    }

    fn copy(&mut self, text: &str, start: usize, end: usize, location: &SourceLocation) {
        append_range(
            self.expanded,
            text,
            SourceSpan { start, end },
            location,
            line_number(text, start),
            &self.chain,
        );
    }
}

fn line_number(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn append_range(
    expanded: &mut Expanded,
    text: &str,
    span: SourceSpan,
    location: &SourceLocation,
    line: usize,
    chain: &[SourceLocation],
) {
    let output_start = expanded.text.len();
    expanded.text.push_str(&text[span.start..span.end]);
    let mut original = location.clone();
    original.span = Some(span);
    expanded.ranges.push(IncludeSourceRange {
        expanded: SourceSpan {
            start: output_start,
            end: expanded.text.len(),
        },
        original,
        line,
        include_chain: chain.to_vec(),
    });
}

fn code_delimiters(format: AuthoredFormat) -> (&'static str, &'static str) {
    if format == AuthoredFormat::Gfm {
        ("<!--@include-code:", "-->")
    } else {
        ("{{< include-code ", ">}}")
    }
}

struct CodeInclude {
    directive: SourceLocation,
    origin: SourceLocation,
    content: String,
    selected: SourceSpan,
    indent_end: usize,
    line_end: usize,
    separate_options: bool,
}

fn fence_marker(code: &CodeBlock, kind: SyntaxKind) -> Option<SourceSpan> {
    let token = code
        .syntax()
        .children()
        .find(|node| node.kind() == kind)?
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::CODE_FENCE_MARKER)?;
    let range = token.text_range();
    Some(SourceSpan {
        start: usize::from(range.start()),
        end: usize::from(range.end()),
    })
}

fn marker_run(line: &str, marker: u8) -> usize {
    line.trim_start_matches([' ', '\t'])
        .bytes()
        .take_while(|byte| *byte == marker)
        .count()
}

fn code_target(inner: &str, format: AuthoredFormat) -> Option<(&str, Option<(usize, usize)>)> {
    let inner = inner.trim();
    let Some((path, range)) = inner.strip_suffix('}').and_then(|s| s.rsplit_once('{')) else {
        return Some((validate_path(inner, format)?, None));
    };
    let (first, last) = range.split_once('-')?;
    let number = |value: &str| {
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| value.parse::<usize>().ok())
            .flatten()
    };
    let (first, last) = (number(first)?, number(last)?);
    (first > 0 && last >= first)
        .then_some((validate_path(path.trim_end(), format)?, Some((first, last))))
}

fn select_lines(content: &str, selection: Option<(usize, usize)>) -> Option<SourceSpan> {
    let Some((first, last)) = selection else {
        return Some(SourceSpan {
            start: 0,
            end: content.len(),
        });
    };
    let mut lines = content.split_inclusive('\n');
    let start: usize = lines.by_ref().take(first - 1).map(str::len).sum();
    let count = last - first + 1;
    let mut end = start;
    for _ in 0..count {
        end += lines.next()?.len();
    }
    Some(SourceSpan { start, end })
}

fn target(raw: &str, format: AuthoredFormat) -> Option<&str> {
    let raw = raw.trim_end();
    let inner = if format == AuthoredFormat::Gfm {
        raw.strip_prefix("<!--@include:")?.strip_suffix("-->")?
    } else {
        raw.strip_prefix("{{< include")?.strip_suffix(">}}")?
    };
    validate_path(inner.trim(), format)
}

fn validate_path(inner: &str, format: AuthoredFormat) -> Option<&str> {
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
    Some(path)
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

pub(crate) fn resource_target(
    target: &str,
    span: SourceSpan,
    page: &SourceLocation,
    ranges: &[IncludeSourceRange],
) -> Option<String> {
    if target.is_empty() || target.starts_with(['#', '/']) || url::Url::parse(target).is_ok() {
        return None;
    }
    let range = ranges
        .iter()
        .find(|range| range.expanded.start <= span.start && span.start < range.expanded.end)?;
    if range.include_chain.is_empty() {
        return None;
    }
    let page_directory = Path::new(page.path.as_str()).parent().unwrap();
    let origin_directory = Path::new(range.original.path.as_str()).parent().unwrap();
    let common = page_directory
        .components()
        .zip(origin_directory.components())
        .take_while(|(left, right)| left == right)
        .count();
    let mut prefix = "../".repeat(page_directory.components().count() - common);
    // Encode only the new prefix. The authored target retains its escaping and
    // traversal so the resolver can validate its original meaning.
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    for component in origin_directory.components().skip(common) {
        prefix.push_str(
            &percent_encoding::utf8_percent_encode(
                component.as_os_str().to_str().unwrap(),
                SEGMENT,
            )
            .to_string(),
        );
        prefix.push('/');
    }
    Some(format!("{prefix}{target}"))
}

pub(crate) fn remap(diagnostic: &mut Diagnostic, ranges: &[IncludeSourceRange]) {
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

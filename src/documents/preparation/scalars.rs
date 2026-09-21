//! Literal values after Panache has checked YAML structure.

use panache_parser::syntax::parse_yaml_document;

use crate::ir::{MetadataValue, SourceSpan};

#[derive(Clone, Copy)]
pub(super) enum Value<'a> {
    Inline(&'a str),
    Yaml(&'a MetadataValue, &'a str),
}

impl Value<'_> {
    pub fn boolean(self) -> Option<bool> {
        let raw = match self {
            Self::Inline(raw) => raw,
            Self::Yaml(MetadataValue::Scalar { raw, .. }, _) => raw,
            _ => return None,
        };
        match raw.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    pub fn string(self) -> Option<String> {
        match self {
            Self::Yaml(MetadataValue::Scalar { raw, value, span }, source)
                if string_scalar(raw) =>
            {
                if raw.starts_with(['|', '>']) {
                    Some(block_string(value, source, *span))
                } else {
                    Some(value.clone())
                }
            }
            Self::Inline(raw) if !raw.is_empty() => {
                let document = parse_yaml_document(raw)?;
                let scalar = document.scalar()?;
                // Tags and anchors decorate the scalar outside its own node.
                if scalar.raw().trim() != raw.trim() || !string_scalar(raw) {
                    return None;
                }
                Some(scalar.value())
            }
            _ => None,
        }
    }

    pub fn strings(self) -> Option<Vec<String>> {
        match self {
            Self::Yaml(MetadataValue::Sequence { items, .. }, source) => items
                .iter()
                .map(|value| Self::Yaml(value, source).string())
                .collect(),
            _ => None,
        }
    }
}

fn string_scalar(raw: &str) -> bool {
    let raw = raw.trim();
    if raw.starts_with(['\'', '"', '|', '>']) {
        return true;
    }
    if raw.is_empty()
        || matches!(
            raw,
            "~" | "null"
                | "Null"
                | "NULL"
                | "true"
                | "True"
                | "TRUE"
                | "false"
                | "False"
                | "FALSE"
                | ".inf"
                | ".Inf"
                | ".INF"
                | "+.inf"
                | "+.Inf"
                | "+.INF"
                | "-.inf"
                | "-.Inf"
                | "-.INF"
                | ".nan"
                | ".NaN"
                | ".NAN"
        )
    {
        return false;
    }
    let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let radix_number = raw
        .strip_prefix("0x")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()))
        || raw
            .strip_prefix("0o")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| (b'0'..=b'7').contains(&b)));
    let decimal = unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        && raw.parse::<f64>().is_ok();
    !radix_number && !decimal
}

fn block_string(value: &str, source: &str, span: SourceSpan) -> String {
    // Panache removes container/hashpipe framing but leaves block scalars raw.
    // Finish only their scalar cooking; syntax validation stays with Panache.
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let (header, body) = normalized.split_once('\n').unwrap_or((&normalized, ""));
    let indicators = header.split_whitespace().next().unwrap_or(header);
    let indent = indicators
        .bytes()
        .find(u8::is_ascii_digit)
        .map(|digit| yaml_indent(source, span) + usize::from(digit - b'0'))
        .unwrap_or_else(|| {
            body.lines()
                .find(|line| !line.trim().is_empty())
                .map_or(0, |line| line.bytes().take_while(|b| *b == b' ').count())
        });
    let lines = body
        .split_inclusive('\n')
        .map(|line| {
            let (line, newline) = line
                .strip_suffix('\n')
                .map_or((line, false), |line| (line, true));
            let spaces = line.bytes().take_while(|b| *b == b' ').count();
            (&line[indent.min(spaces)..], newline)
        })
        .collect::<Vec<_>>();
    let folded = indicators.starts_with('>');
    let mut result = String::new();
    for (index, &(line, newline)) in lines.iter().enumerate() {
        result.push_str(line);
        if !newline {
            continue;
        }
        let normal = |line: &str| !line.is_empty() && !line.starts_with([' ', '\t']);
        let fold = folded
            && normal(line)
            && lines[index + 1..]
                .iter()
                .find(|(line, _)| !line.is_empty())
                .is_some_and(|(line, _)| normal(line));
        if fold {
            if !lines[index + 1].0.is_empty() {
                result.push(' ');
            }
        } else {
            result.push('\n');
        }
    }
    if !indicators.contains('+') {
        let had_break = result.ends_with('\n');
        result.truncate(result.trim_end_matches('\n').len());
        if !indicators.contains('-') && had_break && !result.is_empty() {
            result.push('\n');
        }
    }
    result
}

fn yaml_indent(source: &str, span: SourceSpan) -> usize {
    let prefix = source[..span.start].rsplit('\n').next().unwrap_or("");
    // Supported keys contain no pipe, so a pipe before the scalar belongs to
    // the hashpipe marker. Its optional separating space is host framing.
    let prefix = prefix
        .rsplit_once('|')
        .map_or(prefix, |(_, yaml)| yaml.strip_prefix(' ').unwrap_or(yaml));
    prefix.bytes().take_while(|b| *b == b' ').count()
}

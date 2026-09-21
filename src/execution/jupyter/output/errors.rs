//! Normalize runtime frame locations without rewriting arbitrary authored text.

use std::path::{Component, Path, PathBuf};

use crate::ir::CellOutputKind;

/// Known checkout paths stay local and never become serialized output evidence.
pub(in crate::execution::jupyter) struct ErrorContext {
    repository_root: PathBuf,
}

impl ErrorContext {
    pub fn new(repository_root: PathBuf) -> Self {
        Self { repository_root }
    }

    pub fn normalize(
        &self,
        name: String,
        message: String,
        traceback: Vec<String>,
    ) -> CellOutputKind {
        CellOutputKind::Error {
            name: strip_controls(&name),
            message: strip_controls(&message),
            traceback: traceback
                .into_iter()
                .map(|frame| {
                    strip_controls(&frame)
                        .split('\n')
                        .map(|line| self.frame(line))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect(),
        }
    }

    fn frame(&self, frame: &str) -> String {
        let trimmed = frame.trim_start();
        if let Some(cell) = trimmed.strip_prefix("Cell In[")
            && let Some((count, rest)) = cell.split_once(']')
            && !count.is_empty()
            && count.bytes().all(|byte| byte.is_ascii_digit())
            && rest.starts_with(", line ")
        {
            return format!("{}Cell <cell>{rest}", &frame[..frame.len() - trimmed.len()]);
        }
        let Some(location) = trimmed.strip_prefix("File ") else {
            return frame.into();
        };
        let start = frame.len() - location.len();
        let (offset, length) = if let Some(quoted) = location.strip_prefix('"') {
            let Some(end) = quoted.find('"') else {
                return frame.into();
            };
            (1, end)
        } else {
            // IPython uses an unquoted filename followed by a numeric line.
            let end = location.char_indices().find_map(|(index, ch)| {
                (ch == ':' && location[index + 1..].starts_with(|ch: char| ch.is_ascii_digit()))
                    .then_some(index)
            });
            let Some(end) = end else {
                return frame.into();
            };
            (0, end)
        };
        let path = &location[offset..offset + length];
        let absolute = Path::new(path).is_absolute()
            || path.starts_with("\\\\")
            || (path.as_bytes().get(1) == Some(&b':')
                && path
                    .as_bytes()
                    .get(2)
                    .is_some_and(|ch| matches!(ch, b'/' | b'\\')));
        if !absolute {
            return frame.into();
        }
        let replacement = Path::new(path)
            .strip_prefix(&self.repository_root)
            .ok()
            .filter(|relative| {
                !relative.as_os_str().is_empty()
                    && relative
                        .components()
                        .all(|part| matches!(part, Component::Normal(_)))
            })
            .and_then(Path::to_str)
            .unwrap_or("<external-frame>");
        format!(
            "{}{}{}",
            &frame[..start + offset],
            replacement,
            &frame[start + offset + length..]
        )
    }
}

fn strip_controls(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => consume_csi(&mut chars),
                Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                    consume_string(&mut chars)
                }
                Some(intermediate) if (' '..='/').contains(&intermediate) => {
                    for ch in chars.by_ref() {
                        if ('0'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\u{9b}' => consume_csi(&mut chars),
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => consume_string(&mut chars),
            '\n' | '\t' => clean.push(ch),
            ch if ch.is_control() => {}
            _ => clean.push(ch),
        }
    }
    clean
}

fn consume_csi(chars: &mut impl Iterator<Item = char>) {
    for ch in chars {
        if ('@'..='~').contains(&ch) {
            break;
        }
    }
}

fn consume_string(chars: &mut impl Iterator<Item = char>) {
    let mut escape = false;
    for ch in chars {
        if ch == '\u{7}' || ch == '\u{9c}' || (escape && ch == '\\') {
            break;
        }
        escape = ch == '\u{1b}';
    }
}

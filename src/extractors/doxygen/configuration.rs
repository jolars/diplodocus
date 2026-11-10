//! Tokenization of native-normalized Doxyfile settings and include directives.

use std::collections::BTreeMap;

/// Effective settings emitted by `doxygen -x`, without local execution state.
#[derive(Debug, Clone, Default)]
pub struct DoxygenConfiguration {
    /// Values in native list order, keyed by Doxygen option name.
    pub values: BTreeMap<String, Vec<String>>,
}

impl DoxygenConfiguration {
    /// Read assignment syntax after Doxygen has expanded includes and variables.
    pub fn parse(source: &str) -> Self {
        let mut result = Self::default();
        for line in logical_source(source).lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let append = key.trim_end().ends_with('+');
            let key = key.trim().trim_end_matches('+').trim().to_owned();
            let values = words(value);
            if append {
                result.values.entry(key).or_default().extend(values);
            } else {
                result.values.insert(key, values);
            }
        }
        result
    }

    /// First value, with an explicit native default for omitted settings.
    pub fn value<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.values
            .get(key)
            .and_then(|v| v.first())
            .map_or(default, String::as_str)
    }

    /// Whether a native boolean setting is enabled.
    pub fn enabled(&self, key: &str) -> bool {
        self.value(key, "NO") == "YES"
    }
}

pub(crate) fn logical_source(source: &str) -> String {
    let mut result = String::new();
    for line in source.lines() {
        let line = line.trim_end();
        if let Some(continued) = line.strip_suffix('\\') {
            result.push_str(continued);
            result.push(' ');
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

pub(crate) fn excluded(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".devenv"
            | ".direnv"
            | ".cache"
            | ".diplodocus"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "_site"
            | ".docs-sources"
            | "site"
            | "artifacts"
            | "coverage"
            | "__pycache__"
            | ".playwright-cli"
            | "playwright-report"
            | "test-results"
    )
}

/// Split Doxyfile values without interpreting commands or evaluating expressions.
pub(crate) fn words(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in source.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == '#' && !quoted {
            break;
        } else if ch.is_whitespace() && !quoted {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        result.push(word);
    }
    result
}

/// Expand Doxygen's environment syntax for local configuration dependency discovery.
pub(crate) fn environment(source: &str) -> String {
    let mut result = String::new();
    let mut remaining = source;
    while let Some(start) = remaining.find("$(") {
        result.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        let Some(end) = tail.find(')') else {
            result.push_str(&remaining[start..]);
            return result;
        };
        result.push_str(&std::env::var(&tail[..end]).unwrap_or_default());
        remaining = &tail[end + 1..];
    }
    result.push_str(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_settings_preserve_quoted_lists_and_continuations() {
        let settings = DoxygenConfiguration::parse(
            "PROJECT_NAME = \"C++ # project\" # comment\nINPUT = \"include with spaces\" \\\n other\nINPUT += extra\nEXTRACT_PRIVATE = YES\n",
        );
        assert_eq!(settings.value("PROJECT_NAME", ""), "C++ # project");
        assert_eq!(
            settings.values["INPUT"],
            ["include with spaces", "other", "extra"]
        );
        assert!(settings.enabled("EXTRACT_PRIVATE"));
    }
}

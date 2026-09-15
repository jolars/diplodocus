use std::collections::BTreeMap;
use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::diagnostics::DiagnosticPath;
use crate::ir::Fingerprint;

/// SHA-256 of exact original bytes, without a prefix or newline normalization.
///
/// The existing IR stores the algorithm and lowercase hex value separately;
/// their cache-contract spelling is `sha256:<value>`.
///
/// Repository manifests use `H("diplodocus/declared-inputs-v1", records)`, with
/// the cache contract's NUL-separated domain and canonical JSON encoding.
/// `records` is an array of `{digest, path, repository}` objects, sorted by
/// repository and path byte order. `digest` uses the cache-contract spelling.
/// Only content and portable input identity participate, never Git state,
/// revisions, absolute roots, discovery order, or file metadata.
pub fn fingerprint_bytes(bytes: &[u8]) -> Fingerprint {
    Fingerprint {
        algorithm: "sha256".into(),
        value: format!("{:x}", Sha256::digest(bytes)),
    }
}

pub(super) fn fingerprint_manifest(
    repository: &str,
    inputs: &BTreeMap<DiagnosticPath, Fingerprint>,
) -> Fingerprint {
    let mut bytes = String::from("diplodocus/declared-inputs-v1\0[");
    for (index, (path, digest)) in inputs.iter().enumerate() {
        if index != 0 {
            bytes.push(',');
        }
        bytes.push_str("{\"digest\":");
        string(
            &mut bytes,
            &format!("{}:{}", digest.algorithm, digest.value),
        );
        bytes.push_str(",\"path\":");
        string(&mut bytes, path.as_str());
        bytes.push_str(",\"repository\":");
        string(&mut bytes, repository);
        bytes.push('}');
    }
    bytes.push(']');
    fingerprint_bytes(bytes.as_bytes())
}

fn string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\0'..='\u{1f}' => {
                write!(output, "\\u{:04x}", u32::from(character)).expect("write to string");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_manifest_matches_independent_sha256_vectors() {
        // These digests were computed independently with Python's hashlib.
        let inputs = BTreeMap::from([(
            DiagnosticPath::try_from("nested/a\t\n\"é\u{2028}.py").unwrap(),
            fingerprint_bytes(b"abc"),
        )]);
        assert_eq!(
            fingerprint_manifest("repository", &inputs).value,
            "428d0dc1d3abfc91ae57025569dac03e428062d0b645055d75508cd25ea9e96d"
        );
        assert_eq!(
            fingerprint_manifest("repository", &BTreeMap::new()).value,
            "abe9cd15af688ecae97830268170a72eeffbe4fd2f82453e9446134b2af6629e"
        );
    }
}

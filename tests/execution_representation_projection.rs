//! The shared live/cache adapter must match every immutable representation vector.
use diplodocus::configuration::ExecutionMode;
use diplodocus::documents::{AuthoredFormat, MarkdownFragmentOrigin, parse_markdown_fragment};
use diplodocus::execution::ExecutionPage;
use diplodocus::execution::assets::PageAssetStore;
use diplodocus::execution::output_safety::*;
use diplodocus::execution::validated::{RepresentationContent, project_representation};
use diplodocus::ir::{Fingerprint, SourceLocation, SourceSpan};
use diplodocus::provenance::fingerprint_bytes;
use serde_json::Value;
use std::collections::BTreeSet;

fn fingerprint(value: &Value) -> Fingerprint {
    Fingerprint {
        algorithm: "sha256".into(),
        value: value
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
            .into(),
    }
}
#[test]
fn shared_projection_matches_all_eight_reference_representations() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/spikes/fixtures/execution-artifact-v1");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let identity = &manifest["key_input"]["page"];
    let root = tempfile::tempdir().unwrap();
    let page = ExecutionPage {
        source: SourceLocation {
            repository: identity["repository"].as_str().unwrap().into(),
            path: identity["path"].as_str().unwrap().try_into().unwrap(),
            span: None,
        },
        collection: identity["collection"].as_str().unwrap().into(),
        working_directory: Some(
            identity["working_directory"]
                .as_str()
                .unwrap()
                .try_into()
                .unwrap(),
        ),
        source_fingerprint: fingerprint_bytes(&std::fs::read(fixture.join("source.qmd")).unwrap()),
        format: AuthoredFormat::Qmd,
        mode: ExecutionMode::Execute,
        page_veto: false,
        parser_version: diplodocus::provenance::PANACHE_VERSION.into(),
        qmd_policy: "qmd-mvp-v1".into(),
    };
    let parent = root
        .path()
        .join(page.working_directory.as_ref().unwrap().as_str());
    std::fs::create_dir_all(&parent).unwrap();
    let mut store = PageAssetStore::new(
        page.clone(),
        root.path().into(),
        root.path().join("staging"),
    )
    .unwrap();
    let mut verified = VerifiedAssets::new();
    let figure_digest = manifest["result"]["cells"][0]["outputs"][1]["representations"][0]["content"]["asset"]["digest"].as_str().unwrap();
    for asset in manifest["result"]["assets"].as_array().unwrap() {
        let digest = fingerprint(&asset["digest"]);
        let bytes = std::fs::read(fixture.join("assets/sha256").join(&digest.value)).unwrap();
        let staged = store
            .stage_bytes(asset["media_type"].as_str().unwrap(), &bytes)
            .unwrap();
        verified.stage(&mut store, &staged, &bytes).unwrap();
        std::fs::write(
            parent.join(if asset["digest"] == figure_digest {
                "figure.svg"
            } else {
                "hidden.svg"
            }),
            bytes,
        )
        .unwrap();
    }
    let context = AuthoredOutputContext::new(
        page.source.clone(),
        page.collection.clone(),
        BTreeSet::new(),
    );
    let mut count = 0;
    for cell in manifest["result"]["cells"].as_array().unwrap() {
        for output in cell["outputs"].as_array().unwrap() {
            for representation in output["representations"].as_array().unwrap() {
                let content = &representation["content"];
                let projection = match representation["kind"].as_str().unwrap() {
                    "text" if content["type"] == "literal" => project_representation(
                        RepresentationContent::Text(content["text"].as_str().unwrap()),
                    )
                    .unwrap(),
                    "text" => project_representation(RepresentationContent::Error {
                        name: content["name"].as_str().unwrap(),
                        value: content["value"].as_str().unwrap(),
                        traceback: &serde_json::from_value::<Vec<String>>(
                            content["traceback"].clone(),
                        )
                        .unwrap(),
                    })
                    .unwrap(),
                    "asset" => project_representation(RepresentationContent::Asset(AssetUse {
                        digest: fingerprint(&content["asset"]["digest"]),
                        media_type: content["asset"]["media_type"].as_str().unwrap().into(),
                        byte_size: content["asset"]["byte_size"].as_u64().unwrap(),
                    }))
                    .unwrap(),
                    "unsupported" => project_representation(RepresentationContent::Unsupported(
                        &serde_json::from_value::<BTreeSet<String>>(content["mime_types"].clone())
                            .unwrap(),
                    ))
                    .unwrap(),
                    "html-candidate" => {
                        let producer = representation["producing_cell"].as_u64().unwrap() as usize;
                        let span = serde_json::from_value::<SourceSpan>(
                            manifest["result"]["cells"][producer]["span"].clone(),
                        )
                        .unwrap();
                        let value = restore_html(
                            DecodedHtml {
                                markup: content["markup"].as_str().unwrap().into(),
                            },
                            &OutputOrigin {
                                cell: producer,
                                slot: 0,
                                cell_span: span,
                                fragment: None,
                            },
                            &context,
                            &verified,
                        )
                        .unwrap();
                        project_representation(RepresentationContent::Html(
                            value.canonical_content(),
                        ))
                        .unwrap()
                    }
                    "markdown" => {
                        let fragment = &representation["fragment"];
                        let source =
                            std::fs::read_to_string(fixture.join(if fragment["ordinal"] == 0 {
                                "fragment.md"
                            } else {
                                "hidden-fragment.md"
                            }))
                            .unwrap();
                        let producer = representation["producing_cell"].as_u64().unwrap() as usize;
                        let span = serde_json::from_value::<SourceSpan>(
                            manifest["result"]["cells"][producer]["span"].clone(),
                        )
                        .unwrap();
                        let origin = OutputOrigin {
                            cell: producer,
                            slot: fragment["slot"].as_u64().unwrap() as usize,
                            cell_span: span,
                            fragment: Some(FragmentIdentity {
                                ordinal: fragment["ordinal"].as_u64().unwrap() as usize,
                                byte_length: source.len(),
                            }),
                        };
                        let parsed = parse_markdown_fragment(
                            &source,
                            MarkdownFragmentOrigin {
                                collection: page.collection.clone(),
                                cell: producer,
                                output: origin.slot,
                                source: SourceLocation {
                                    span: Some(span),
                                    ..page.source.clone()
                                },
                            },
                        );
                        let Validation::Accepted { value, .. } =
                            validate_markdown_live(parsed, &origin, &context, &mut store).unwrap()
                        else {
                            panic!("reference fragment rejected")
                        };
                        project_representation(RepresentationContent::Markdown(
                            value.canonical_content(),
                        ))
                        .unwrap()
                    }
                    other => panic!("unexpected kind {other}"),
                };
                assert_eq!(projection.kind, representation["kind"].as_str().unwrap());
                assert_eq!(
                    serde_json::from_slice::<Value>(&projection.content.encode().unwrap()).unwrap(),
                    *content
                );
                assert_eq!(
                    projection.fingerprint,
                    fingerprint(&representation["content_digest"])
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 8);
}

# Workspace resolution and static checking

This checkpoint connects source assembly to the real `diplodocus check` command
and prepares portable reference and asset records for snapshot publication.
The timeout and watched-site roadmap item remains open: snapshot storage,
generation, and watched publication are still required.

`check` loads configuration, resolves declared roots, runs the static Python and
R extractors, prepares authored content, merges fragments, resolves concepts and
document references, and validates known relationship constraints. It reports
warnings on success and source diagnostics on failure. It neither discovers
kernels nor reads an execution cache, executes cells, stages assets, or creates
snapshot and site output. Library callers receive the ordered diagnostic report.

Semantic resolution shares one implementation with concept resolution. An exact
item ID wins within a selected package. Named callable references select their
family rather than becoming ambiguous with overloads. Qualified references use
their explicit package; other references prefer the owning package, then require
one workspace match. Missing and ambiguous references fail checking.

Local page links resolve to semantic page IDs. Resolved document records retain
selected collection-relative filenames, so generation need not derive routes
from opaque page IDs or canonical source aliases. Anchors come only from authored
content. The reference walker covers nested prose, lists, callouts, tables, and
generated Markdown alternatives. Generated-reference diagnostics identify the
producing or updating authored cell instead of presenting fragment-relative
offsets as source-file offsets.

Local images and downloads must remain beneath declared content or package
roots. File aliases retain their selected path and canonical referent for final
revalidation. Asset bytes are copied into memory and deduplicated by SHA-256;
an image and download with identical bytes share the validated image type.
Remote images, active URLs, missing pages, and missing anchors fail validation.
External HTTP, HTTPS, and mailto hyperlinks remain unfetched. Encoded filenames
and URL delimiters retain their distinct meanings. Page routes remain generation
work.

Checked-in PNG and JPEG use the existing complete image validators. Checked-in
SVG additionally permits inert IDs and ARIA labels, as used by the acceptance
diagram. The generated SVG policy is unchanged. Both reject active content,
external references, style attributes, and embedded HTML.

Executed resolution copies final generated assets only after checking their
recorded fingerprint, size, and active image policy. The copied bytes outlive
execution staging. Immutable engine wrappers remain with the execution owner;
portable records and resolution results never grant HTML rendering trust. A
future snapshot loader must validate serialized output against the active
policies independently.

Relationship constraints use the destination package's version. Python uses
[PEP 440 specifiers](https://docs.rs/pep440_rs/0.7.3/pep440_rs/), R supports
comma-separated numeric comparisons with dot or dash components, and Cargo uses
[SemVer requirements](https://docs.rs/semver/1.0.28/semver/struct.VersionReq.html).
An explicit caret constraint selects SemVer across ecosystems, matching the
existing acceptance declarations. A known mismatch is an error. Unknown
versions or unsupported syntax produce a warning. Absent external destinations
retain their coordinates without requiring a checkout.

Publication consumers must revalidate both source assembly and asset
observations after execution. Asset observations do not yet amend the assembly
collector's repository input fingerprint; snapshot assembly must include their
portable source evidence in its final provenance. The current result is an
input to that transaction, not a published snapshot.

## Validation

All commands below ran in `/home/jola/projects/diplodocus`, with non-login Bash.
The selected policy was `use_default` except for the explicitly approved final
rerun described below. Development commands used the documented `devenv shell
--` entry. Git metadata remains a separate authorization boundary. No running
validation was restarted because an observation call yielded.

| Command or run | Result and diagnostic |
| --- | --- |
| `devenv shell -- cargo test --locked --test workspace_resolution`, before implementation | Expected compile failure: the resolver API and diagnostic variants did not exist. |
| `devenv shell -- cargo test --test workspace_resolution`, while adding pinned dependencies | Lockfile gained direct `pep440_rs` and `semver` dependencies; implementation compilation identified two incorrect API assumptions, then corrected them. |
| Repeated locked resolution runs | Initial acceptance SVG failed the generated-image allowlist. Separate authored accessibility support fixed it. Regression tests then demonstrated dropped anchor observations, undetected alias retargeting, and an encoded filename incorrectly split as a fragment before their fixes. |
| `devenv shell -- cargo test --locked --test check_workspace`, before command integration | After correcting the test assertion API, both tests failed behaviorally against the old `CheckNotImplemented` stub. |
| Focused generated-reference test below, before its fix | Failed: a fragment range was reported as an authored file range. The test now also covers a display update from a later cell. |
| Focused duplicate-asset test below, before its fix | Failed: a download alias conflicted with the identical image bytes' media type. |
| Combined focused command below | Passed: 12 resolution, 11 assembly, 2 real-check, 5 CLI, and 1 command tests. Entry hooks passed. |
| First full repository validation below | Formatting and Clippy passed; all-target tests stopped at the dogfood goldens after the three guide status paragraphs changed. |
| `devenv shell -- bash -c 'SNAPSHOTS=overwrite cargo test --locked --test acceptance_gate real_documentation_matches_authored_goldens'` | Passed. Reviewed all three updated goldens: only the intended paragraph and consequent byte spans differ; block counts and empty diagnostics remain unchanged. |
| Full validation with output redirected to `/tmp/diplodocus-resolution-full-validation.log`, `use_default` | Blocked before checks: Nix could not connect to `/nix/var/nix/daemon-socket/socket` while creating a GC root (`Operation not permitted`). No `.devenv` write denial occurred. |
| Same logged full validation, explicitly approved `require_escalated` | Passed, exit 0: formatting, all-target/all-feature Clippy with warnings denied, 593 all-target tests, 27 doctests, and rustdoc with warnings denied. |
| `devenv shell -- cargo deny check` | Failed, exit 4: advisories, bans, and sources passed; the same ten baseline license-policy entries failed. |
| `git diff --check` | Passed. |

Some intermediate development entries let rustfmt update newly added code and
reported the hook modification as a failure. Final validation checks formatting
without changes. Compiler errors and expected red tests above were implementation
feedback, not validation-environment blockers.

The license failures match the package names and versions recorded in the
[earlier baseline](execution-m6-04-validation.md#coordinator-validation-of-the-recovered-implementation):
`ar_archive_writer 0.5.3`, `jupyter-protocol 2.0.2`, `jupyter-zmq-client 1.0.1`,
`option-ext 0.2.0`, `ring 0.17.14`, `sha1_smol 1.0.1`, `unicode_names2 1.3.0`,
`untrusted 0.9.0`, `version-ranges 0.1.3`, and `win_uds 0.2.2`. Neither their
versions nor `deny.toml` changed. The new `semver` dependency passed policy;
`pep440_rs` was already present transitively. This remains a failed check, not
an overall validation pass or a license-policy exception.

A durable Coterie report addressed to the current lead coordinator was rejected
because the lead cannot send a message to itself (operation
`co-01M37XDGSGRHXVSA1FA01Q66WK`). All other listed peers had exited. This committed
record retains the evidence for the operator; no additional agent or authority
was created to route the report.

```sh
devenv shell -- cargo test --locked --test workspace_resolution generated_reference_errors_point_to_the_authored_cell
devenv shell -- cargo test --locked --test workspace_resolution identical_image_and_download_bytes_share_one_asset
devenv shell -- cargo test --locked --test workspace_resolution --test workspace_assembly --test check_workspace --test cli --test commands
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The command tests compare complete directory and file-byte inventories before
and after successful and failed checking. They use missing kernel selectors,
executable sentinel code, and a CLI process with an empty `PATH`. Repeated errors
produce identical stderr. Relocated acceptance workspaces produce identical
portable reference records, and real Python/R execution results resolve before
their staging is explicitly discarded.

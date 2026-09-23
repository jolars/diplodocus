# Workspace source assembly and execution ownership

This checkpoint connects the existing source adapters and public execution
engine as a prerequisite for the timeout and watched-site request. It does not
publish a snapshot or site, and the compound roadmap item remains open.

`assemble_workspace` loads and validates configuration, resolves declared
boundaries, invokes the selected Python and R extractors, and prepares authored
pages without executing them. It merges package fragments, rejects overlapping
item identities, resolves concept members to canonical items or callable
families, and retains relationship coordinates and declaration provenance.
Authored discovery sorts selected files, ignores `.git` and `.diplodocus`, and
rejects directory symlinks. File reads reuse repository containment checks.

The result owns the original source and preparation alongside portable workspace
records. Page IDs encode collection identity and collection-relative path with
length prefixes. Parser and extractor input fingerprints must agree with a fresh
static provenance collection. Configuration-relative paths keep the caller's
directory when the configuration file is a symlink. Missing Git observations
retain the existing collector's unknown values rather than inferred revisions.

The separate Linux `WorkspaceSources::execute` operation requires its caller to
authorize the current command. It runs eligible pages in collection declaration
order and relative-path order, with one supervised engine call per page. Never,
vetoed, and candidate-free pages do not discover kernels or create staging.
The result keeps each engine's immutable validation evidence and projects its
portable cell outputs and provenance into the corresponding authored document.

One lazily created temporary directory owns every page's staging. A later page
failure removes assets from earlier successful pages; a dropped future also
discards the attempt's staging while the engine supervisor reaps the active
kernel. Explicit disposal reports cleanup failure. On failure, cleanup errors
retain the original cause. Cancellation is checked before the attempt, between
pages, and after final input revalidation, including when there are no executable
cells.

Final revalidation checks the configuration, resolved roots, authored file set,
and bytes of selected sources and declared environment inputs. This catches a
later page changing an earlier executed source. It detects ordinary changes,
not an operating-system snapshot of undeclared inputs or subsequent edits.

This is source assembly, not final workspace validation. Document semantic and
local references, relationship version constraints, checked-in asset collection,
snapshot validation and publication, site generation, and watched command
integration still need implementation. No command stub has been labeled as a
working check, build, extract, generate, or serve operation.

## Validation

Every command below ran in `/home/jola/projects/diplodocus`, in a non-login shell,
with selected policy `use_default`, using the documented `devenv shell --` entry.
There was no Nix daemon or `.devenv` write blocker. Git metadata access remains
a separate authorization boundary. Compiler and build-lock activity was observed
through its existing process handle; no running validation was restarted because
of an observation timeout.

| Command/run | Result and diagnostic |
| --- | --- |
| `devenv shell -- cargo test --locked --test workspace_assembly`, before implementation | Expected red: the assembly module and conflict diagnostic did not exist. |
| Same command, initial implementation | Five tests passed. The entry Clippy hook rejected a map lookup followed by insertion; the merge now uses the entry API. |
| Focused cancellation command below, before adding the error variant | Expected compile failure for the new cancellation variant. |
| Same cancellation command, before implementing cancellation checks | Expected behavioral failure: an already-canceled attempt with no executable cells returned success. |
| `devenv shell -- cargo test --locked --test workspace_assembly`, after cancellation changes | Nine tests passed, including cancellation and dropped futures with an active real kernel and prior staged assets. Entry hooks passed. |
| Focused symlink command below, before the fix | Expected red: revalidation incorrectly moved relative roots to the symlink target's parent. |
| `devenv shell -- cargo test --locked --test workspace_assembly`, after the fix and corpus test | Eleven tests passed. Entry Clippy and formatting hooks passed. |
| Full validation command below | Passed: formatting, all-target/all-feature Clippy with warnings denied, 578 all-target tests, 27 documentation tests, and rustdoc with warnings denied. |
| `git diff --check` | Passed. |

```sh
devenv shell -- cargo test --locked --test workspace_assembly cancellation_before_a_no_execution_attempt_prevents_success
devenv shell -- cargo test --locked --test workspace_assembly configuration_symlink_keeps_the_callers_configuration_directory
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The tests compare relocated acceptance workspaces, reject overlapping targets and
invalid source/concept declarations, verify no staging or discovery for disabled
paths, and execute the four declared Python/R pages in the acceptance corpus.
They also check retained typed output, workspace-owned asset disposal, rollback
after a later execution failure, detection of cross-page input mutation, and
process reaping after cancellation and dropping the active attempt.

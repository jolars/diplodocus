# Complete execution artifact reference

`manifest.json` contains canonical `execution-json-v1` bytes, without a trailing
newline. Together with `assets/sha256/*`, it is one complete synthetic cache
entry. Copy only those paths into a cache entry; this README, `source.qmd`,
`fragment.md`, `hidden-fragment.md`, and `cleared-fragment.md` are test inputs outside the entry.

The [integration contract](../../../design/execution-integration-contracts.md)
defines the exact nested shapes. The original
[key vector](../execution-cache-key-v1.json) remains unchanged. This fixture
reuses its synthetic engine, launch, and runtime observations; component names
and parser versions describe the representation adapters used here. They are
not claims about an installed execution environment. No kernel ran to produce
this artifact. Its output sequence is a protocol scenario independent of what
the short illustrative cell sources would do in a real interpreter.

The four cells demonstrate an updated display in cell 0 at slot 2, a separate
hidden Markdown output in cell 1, an unsupported display followed by an allowed
error in cell 2, and an eval-disabled cell 3. Cell 0 clears its initial Markdown slot 0, then retains stream slot 1 and
display slot 2. The warning from its cleared fragment survives. Cell 1 updates cell 0 before producing its own slot 0; both
fragments therefore use producer slot 0 but have distinct fragment ordinals.
The two images in the list bind independently to the same asset. The hidden
fragment references the second asset, which no top-level asset representation
uses. Removing it would make the artifact incomplete.

One realizing event trace is: cell 0 displays `cleared-fragment.md` at slot 0,
clears its output immediately, emits stdout at slot 1, and displays a registered
bundle at slot 2. Cell 1 updates that registration, then displays
`hidden-fragment.md` at its own slot 0. Cell 2 displays a rejected HTML candidate
at slot 0 and reports its allowed exception at slot 1. The original registration
and cleared content are absent from the final artifact; their warnings remain.

The typed warning ledger contains a producing-session message warning, a cleared
fragment warning, and an HTML rejection warning, with local indices 0, 1, and 2. Replaying a cache entry does not
replay its original discovery diagnostics. `tests/execution_artifact_contract.rs`
checks actual parser/preparation output, canonical digests, asset validation and
closure, slot identity, and diagnostic references. It is not a cache decoder or
HTML safety implementation.

The fixed digests are:

```text
key: sha256:66a19f4d996779c744f94c2b880ef6235130bdf79ad62ef4abc9391c8ee0c661
result: sha256:e067aefd02d8dbee7c20f230959e28cf9c04e19bfeb39e92cb7874f863db857d
```

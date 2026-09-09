# Workspace configuration

The root `diplodocus.toml` is an authored-only example of the intended MVP
configuration. Configuration loading and command execution remain under
development.

## Repositories and ownership

Repository paths are relative to the configuration directory and may identify
sibling checkouts. Package paths are relative to their repository, and API
target paths are relative to their package. Authored content and declared
environment inputs are relative to the named repository.

The `project` owner places content in project navigation. A package ID gives a
collection package ownership and that package's reference-resolution context.
Package documentation lives under `/packages/<slug>/`.

## The project site

This configuration declares one repository and two project-owned collections:

| Collection | Source | Mount | Profile | Execution |
|:-----------|:-------|:------|:--------|:----------|
| Guide | `docs/guide` | `guide` | GFM | Never |
| Examples | `docs/examples` | `examples` | QMD | Python |

The guide's omitted execution settings default to `never`. The examples
collection explicitly declares:

```toml
[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared_environment_inputs = ["devenv.lock"]
```

The declared lockfile contributes to provenance and execution-cache keys.
Diplodocus does not interpret it as an installation instruction. One page uses
one kernel session, with its cells run in source order.

## Profiles and assets

GFM collections discover `.md` files. QMD collections discover `.qmd` files and
support executable fences, hashpipe options, and callouts. Unsupported syntax
produces a visible diagnostic. Raw authored HTML is escaped or represented by
an unsupported node.

Keep checked-in assets beside their owning content. Generated figures become
local content-addressed assets, and generated output cannot read assets outside
its declared boundary. Output directories and execution caches are not inputs
to watched builds.

Return to the [overview](index.md) or try the [quick start](quick-start.md).

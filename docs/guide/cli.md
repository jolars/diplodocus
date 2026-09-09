# Commands

The command-line options are available today. The operations below describe
their MVP behavior; the current binary reports that each operation is not yet
implemented.

| Command | Expected operation |
|:--------|:-------------------|
| `check` | Load, parse, extract, and validate without executing cells or writing a site. |
| `build` | Validate, run configured execution, and render a static site. |
| `serve` | Build, serve over HTTP, watch declared inputs, and rebuild safely. |

All three commands accept `--config`, defaulting to `./diplodocus.toml`.
`build` and `serve` accept `--output`, defaulting to `./site`.
`serve` also accepts `--host` and `--port`, defaulting to `127.0.0.1` and `8000`.
Relative command-line paths are resolved from the working directory.

Warnings remain visible and permit a successful exit. Errors cause a nonzero
exit status. Failed builds do not replace a successful output tree. During
preview, a failed rebuild prints its diagnostics while serving the last
successful site. Correcting the input allows the next rebuild to succeed.

The watcher observes configuration, API and metadata sources, authored pages,
assets, and declared environment inputs. Output, execution-cache, and unrelated
file changes do not trigger rebuilds. Browser live reload is outside the MVP.

See the [quick start](quick-start.md) for complete acceptance and project-site
commands.

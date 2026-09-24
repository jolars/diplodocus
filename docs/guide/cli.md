# Commands

The commands share validation and publication stages:

| Command | Operation |
|:--------|:----------|
| `check` | Load, parse, extract, and validate without executing cells or publishing output. |
| `extract` | Run configured execution and publish a portable SQLite snapshot. |
| `generate` | Render a completed snapshot without source checkouts or language runtimes. |
| `build` | Extract a snapshot, then generate a static site from it. |
| `serve` | Build, serve over HTTP, watch declared inputs, and rebuild safely. |

`check`, `extract`, `build`, and `serve` accept `--config`, defaulting to
`./diplodocus.toml`. Extraction publishes `.diplodocus/documentation.sqlite`
beside the configuration file. `extract --output PATH` overrides that location.

`generate` requires `--input PATH`. `generate`, `build`, and `serve` accept
`--output`, defaulting to `./site`. `serve` also accepts `--host` and `--port`,
defaulting to `127.0.0.1` and `8000`. Relative command-line paths resolve from
the working directory. Output destinations must not replace declared inputs;
site destinations must be empty or contain an earlier Diplodocus site.

```text
diplodocus extract --output documentation.sqlite
diplodocus generate --input documentation.sqlite --output site
```

A copied snapshot contains the structured documentation, provenance, and local
asset bytes required for generation. Generation validates stored records and
output against the active policies before publishing.

Warnings remain visible and permit a successful exit. Errors cause a nonzero
exit status. Failed builds do not replace a successful output tree. During
preview, a failed rebuild prints its diagnostics while serving the last
successful site. Correcting the input allows the next rebuild to succeed.

The watcher observes configuration, API and metadata sources, authored pages,
assets, and declared environment inputs. Output, execution-cache, and unrelated
file changes do not trigger rebuilds. The watcher polls content fingerprints and debounces edits for 200 milliseconds.
Browser live reload is outside the MVP.

Execution has a 30-second startup limit, a 60-second cell limit, and a 5-second
limit for synchronizing a reply with idle status. Output activity does not
extend these deadlines. Interruption, shutdown, termination, and forced reaping
each have a 5-second limit. The library accepts explicit `ExecutionDeadlines`
for callers that need different limits. During preview, Ctrl-C or SIGTERM
requests cancellation and waits for supervised kernel cleanup.

See the [quick start](quick-start.md) for complete acceptance and project-site
commands.

# Commands

| Command | Operation |
|:--------|:----------|
| `check` | Validate declared sources without executing cells or publishing output. |
| `extract` | Extract sources, run authorized cells, and publish a portable SQLite snapshot. |
| `generate` | Render a completed snapshot without source checkouts or language runtimes. |
| `build` | Extract a snapshot, then generate a static site. |
| `serve` | Build, serve over HTTP, and rebuild when declared inputs change. |

`check`, `extract`, `build`, and `serve` use `./diplodocus.toml` unless given
`--config PATH`. Extraction writes `.diplodocus/documentation.sqlite` beside
that configuration; `extract --output PATH` changes the snapshot destination.
`generate` requires `--input PATH`. `generate`, `build`, and `serve` write to
`./site` unless given `--output PATH`. Relative command-line paths resolve from
the working directory.

For example, extract once and generate from the completed snapshot:

```console
diplodocus extract --output documentation.sqlite
diplodocus generate --input documentation.sqlite --output site
```

The snapshot includes structured documentation, provenance, and local assets.
Generation validates its records and output before publication. A copied
snapshot can be generated without the original source files or language
runtimes.

`serve` listens on `127.0.0.1:8000` by default. Use `--host` and `--port` to
change the address, and `--live-reload` to refresh open pages after a successful
rebuild. A failed rebuild prints diagnostics and keeps serving the last
successful site. Warnings permit a successful exit; errors cause a nonzero exit
status. Output destinations cannot replace declared inputs.

Start with the [small example](quick-start.md) or see its
[workspace configuration](configuration.md).

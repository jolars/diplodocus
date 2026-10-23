`check`, `extract`, `build`, and `serve` use `./diplodocus.yaml` unless given
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
runtimes. See [portable snapshots](../guide/snapshots.md) for compatibility and refresh
behavior.

Use `extract --watch` to prepare snapshots continuously for another frontend.
It extracts once, then watches declared sources, configuration, and referenced
assets, including sibling package checkouts. Saves are debounced, and extractions
run one at a time. Each successful publication writes and flushes one JSON line
to stdout:

```json
{"format_version":1,"event":"snapshot","path":"/absolute/path/documentation.sqlite"}
```

Diagnostics go to stderr. An initial failure exits without emitting an event.
Later failures retain the previous snapshot and keep watching for corrections.
Consumers should use publication events rather than observing temporary database
files. If a consumer needs several passes over a snapshot, copy it before
processing so a subsequent publication cannot change its input between passes.
Stopping the watcher waits for active example execution to clean up. The
[VitePress example](https://github.com/jolars/diplodocus/tree/main/examples/vitepress)
connects these events to an existing frontend's development server.

`serve` listens on `127.0.0.1:8000` by default. Use `--host` and `--port` to
change the address, and `--live-reload` to refresh open pages after a successful
rebuild. A failed rebuild prints diagnostics and keeps serving the last
successful site. Warnings permit a successful exit; errors cause a nonzero exit
status. Output destinations cannot replace declared inputs. The [diagnostics
guide](../guide/diagnostics.md) explains reported codes and failed builds.

Start with the [small example](../guide/quick-start.md) or see its [workspace
configuration](../guide/configuration.md).

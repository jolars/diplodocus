`check`, `extract`, `build`, and `serve` use `./diplodocus.yaml` unless given
`--config PATH`. Extraction writes `.diplodocus/documentation.sqlite` beside
that configuration; `extract --output PATH` changes the snapshot destination.
`generate` requires `--input PATH`. `generate`, `build`, and `serve` write to
`./site` unless given `--output PATH`. Relative command-line paths resolve from
the working directory.

Commands report their active stages, packages or pages, and elapsed time on
stderr. Interactive terminals show progress bars for counted work and spinners
for other stages. CI and redirected stderr receive plain stage summaries.
Completed builds and generation report the page count and output destination.

Use `--no-progress` for stage summaries without animation, or `-q` or `--quiet`
to retain only warnings and errors. These options do not suppress snapshot
publication events on stdout. `--color auto|always|never` controls colors in
help, diagnostics, and status messages. Automatic colors honor a nonempty
`NO_COLOR` variable and turn off in CI, redirected output, and `TERM=dumb`.
Explicit `--color always` also enables colors in redirected output. Global
options work before or after the subcommand.

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
change the address. The ready message shows a clickable URL in terminals that
support hyperlinks and a plain URL elsewhere. Use `--live-reload` to refresh
open pages after a successful rebuild. Add `--open` to open the local URL in
your default browser once the first build succeeds and the server is ready.
With `--port 0`, the URL uses the assigned
port; wildcard hosts (`0.0.0.0` or `::`) open through the corresponding loopback
address. Browser opening is opt-in, requires no terminal input, and uses the
system launcher (`xdg-open` on Linux, `open` on macOS, or `rundll32.exe` on Windows).
A launcher failure prints a warning and leaves preview running. Startup failures
do not open a browser. A failed rebuild prints diagnostics and keeps serving the
last successful site. Warnings permit a successful exit; errors cause a nonzero exit
status. Output destinations cannot replace declared inputs. The [diagnostics
guide](../guide/diagnostics.md) explains reported codes and failed builds.

Start with the [small example](../guide/quick-start.md) or see its [workspace
configuration](../guide/configuration.md).

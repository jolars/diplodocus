# Jupyter execution spike

## Outcome

Use [`jupyter-zmq-client` 1.0.1](https://docs.rs/jupyter-zmq-client/1.0.1/)
with its Tokio runtime and
[`jupyter-protocol` 2.0.2](https://docs.rs/jupyter-protocol/2.0.2/) behind a
Diplodocus-owned execution adapter. The crates are complementary rather than
alternative implementations:

- `jupyter-protocol` supplies typed messages, parent headers, kernel metadata,
  error records, and MIME bundles without choosing a transport;
- `jupyter-zmq-client` supplies kernelspec discovery and parsing, launch-command
  construction, authenticated ZeroMQ framing, and connections for the shell,
  IOPub, control, stdin, and heartbeat channels; and
- Diplodocus must own the kernel process, page and cell state machines,
  timeouts, interruption policy, output collation, display updates, cleanup,
  and conversion into portable document IR.

The exact versions are intentional. Version 1.0.1 of the ZMQ client depends on
version 2.0.2 of the protocol crate, so direct use of the protocol types does
not introduce a duplicate version. Both crates should remain behind the same
adapter because neither exposes the page-execution abstraction required by
Diplodocus.

This spike selects the client stack; it does not move the Milestone 6 execution
engine into test code. The executable probe uses the ZMQ client's in-process
test kernel and never starts a Jupyter server. Testing the declared `python3`
and `ir` kernel processes belongs to the following Milestone 2 task.

## Boundary

```text
QMD CodeCell[]
      │
      ▼
Diplodocus page executor ── timeout, policy, state, cleanup, provenance
      │
      ├── jupyter-zmq-client ── kernelspecs, subprocess command, ZMQ channels
      │            │
      │            └── jupyter-protocol ── wire messages and MIME bundles
      │
      ▼
structured Output[] ── Diplodocus MIME selection, sanitization, and IR
```

A kernel session is page-scoped. The executor sends one `execute_request` for
each authored cell in source order and associates replies by
`parent_header.msg_id`. It retains IOPub output order until the matching
`status: idle`, while also requiring the matching shell `execute_reply`. Channel
order alone cannot associate messages with a cell, and ordering between the
shell and IOPub channels is not meaningful.

## Capability comparison

| Required capability | `jupyter-protocol` | `jupyter-zmq-client` | Diplodocus-owned work | Result |
| --- | --- | --- | --- | --- |
| Kernel discovery | Typed `JupyterKernelspec` | Reads standard Jupyter data directories; optional helpers also invoke `jupyter --paths --json` to find virtual-environment paths | Select a configured name, report malformed or duplicate specs, and record the selected spec | Select with adapter |
| Kernel startup | Typed connection information | Reserves ports and turns a kernelspec into a Tokio command | Write and remove the connection file, spawn and supervise the child, choose its working directory, await readiness, and capture launch failures | Select with adapter |
| Ordered cell execution | `ExecuteRequest`, `ExecuteReply`, `Status`, `ExecuteInput`, and parent headers | Authenticated shell and IOPub connections preserve their channel streams | Sequential page state machine, parent-ID filtering, terminal conditions, and allowed-error policy | Select |
| Streams and errors | Typed stdout/stderr `StreamContent`, `ErrorOutput`, and reply errors | Carries them on IOPub and shell | Preserve order, normalize tracebacks, and convert into portable output IR | Select |
| MIME bundles | Typed plain text, Markdown, HTML, SVG, raster, JSON, common vendor media, and an `Other` fallback; caller-supplied ranking | Carries `display_data` and `execute_result` | Define the preference order, decode assets, sanitize or reject active content, and retain safe fallbacks | Select |
| Display updates | `display_id`, `DisplayData`, `UpdateDisplayData`, and `ClearOutput` | Carries the IOPub messages | Maintain the page's display-ID map and implement replacement and clearing semantics | Select with adapter |
| Timeout | No execution deadline | Reads and writes await indefinitely, except the IOPub-welcome helper | Wrap every startup, request, heartbeat, interrupt, shutdown, and child wait in Tokio deadlines | Consumer-owned |
| Interruption | Typed message-mode interrupt request and reply | Exposes the control channel and retains the kernelspec's `interrupt_mode` string | Send the control message for message mode; signal and supervise the child process group for signal mode; define escalation after a timeout | Consumer-owned |
| Shutdown | Typed shutdown request and reply | Exposes the control channel | Await a graceful reply and process exit, then terminate or kill on deadline, reap the child, and remove the connection file | Select with adapter |

## Executable probe

[`tests/jupyter_execution_spike.rs`](../../tests/jupyter_execution_spike.rs)
exercises the published crate versions in three layers:

1. A disposable Jupyter data directory contains `python3`, `ir`, and malformed
   kernelspecs. The client discovers the two valid specs, retains the interrupt
   mode, silently skips the malformed file, and substitutes the connection-file
   argument when it builds a launch command.
2. Protocol JSON containing plain text, Markdown, HTML, SVG, and an unknown MIME
   type decodes without loss. A display ID survives both initial display data
   and its update. A separate client and kernel connection exchange a
   message-mode interrupt request and reply over the authenticated control
   channel.
3. The Python and R stateful QMD fixtures are parsed through the production
   Panache adapter. Their five exact cell sources are sent in order through real
   local ZMQ sockets to the crate's deterministic test kernel. The probe receives
   ordered stdout and stderr, Markdown and SVG MIME bundles, a display update,
   and a typed error, with execution counts and parent IDs intact. It also proves
   that a stalled IOPub read can be canceled by a Tokio deadline and that a
   control-channel shutdown completes cleanly.

The test kernel returns canned messages for exact source strings. It proves the
Rust message and transport boundary without pretending to verify Python or R
semantics. In particular, its canned error output is paired with an `ok` shell
reply. Real-kernel tests must require the kernel's actual reply status as well as
its IOPub error and must verify that later execution follows Diplodocus's
allowed-error policy.

## Findings by acceptance construct

| Acceptance construct | Available from the selected crates | Diplodocus-owned work |
| --- | --- | --- |
| Five source-ordered cells sharing state | Requests, execution counts, parent IDs, busy/idle status, and shell replies | Keep one kernel alive for the page and send cells strictly in source order |
| stdout and stderr | `Stdio::{Stdout, Stderr}` and unmodified text | Store typed stream events without parsing their contents as Markdown |
| Markdown-valued result | `MediaType::Markdown` in either display data or an execution result | Parse only the selected value as a non-executable fragment |
| SVG figure with text fallback | `MediaType::Svg` and `MediaType::Plain` coexist in one bundle | Apply preference, validate and store the SVG as an execution asset, and attach authored alt text |
| Controlled execution error | Exception name, value, traceback, and reply status are available | Normalize traceback paths and decide whether the cell or page fails |
| Generated Markdown fence | Transport preserves the Markdown string exactly | Fragment parsing must disable execution and semantic target creation |
| Markdown-looking stdout | Stream text remains a string | Escape it as preformatted text and never route it through a Markdown parser |
| Unsafe HTML | `MediaType::Html` is typed but deliberately unsanitized | Reject or sanitize it before document IR; the crate establishes no trust boundary |
| Generated relative asset reference | Markdown and kernel metadata survive transport | Resolve it only within the declared execution boundary and reject traversal |
| Display replacement | Initial and update messages retain the same display ID | Replace the prior logical output without changing unrelated protocol order |

## Gaps and adapter rules

Kernelspec discovery is deliberately forgiving: the list API skips unreadable
directories and malformed `kernel.json` files. Diplodocus needs a diagnostic for
the configured kernel, so production code should resolve the requested name and
surface its read or decode failure rather than treating an invalid spec as
absent. Static discovery does not include a Python environment's `sys.prefix`;
the alternative helper discovers it by running `jupyter --paths --json`. The
execution toolchain decision must choose one documented search strategy and
record every searched path without placing absolute paths in portable output.

`KernelspecDir::command` substitutes `{connection_file}` and copies declared
environment entries, but it is not a kernel manager. It does not expand
`${ENV_VAR}` references in kernelspec environment values, create a connection
file, start or monitor a child, establish a process group, deliver signal-mode
interrupts, or guarantee cleanup. The production adapter must implement these
operations and validate that the kernelspec has a connection-file placeholder.

The transport exposes independent asynchronous connections. It does not provide
a combined request/response future, filter unrelated broadcasts, impose a
deadline, collect outputs, apply `clear_output`, or update an existing display.
The page executor must therefore have an explicit state machine. Unknown message
and MIME variants must yield a visible diagnostic or safe fallback rather than a
panic.

MIME data remains kernel-authored, untrusted input. The protocol crate's typed
HTML, JavaScript, SVG, JSON, and vendor variants describe representation, not
safety. MIME preference and sanitization stay above the Jupyter adapter so the
renderer never accepts a protocol value as trusted HTML.

## Consequences for implementation

The production adapter should depend directly on both selected crates and
convert immediately into Diplodocus-owned execution types. It should use Tokio,
keep the child handle and every channel in one page-session owner, and run a
bounded shutdown path even when startup, execution, or output conversion fails.

Implementation should proceed in this order:

1. Verify real `python3` and `ir` kernels in the declared devenv and CI
   environments without a server or runtime installation during the test.
2. Define normalized execution options, MIME preference, sanitization, failure
   policy, toolchain requirements, and provenance before adding production
   transport code.
3. Specify the page-session state machine, including parent-ID filtering,
   allowed errors, display replacement, stdin rejection, deadlines, interrupt
   escalation, and unconditional shutdown.
4. Convert protocol messages into portable output IR before rendering or cache
   design makes crate-specific types persistent.
5. Add real-kernel golden output for Python and R, then keep the in-process
   transport probe as the deterministic lower layer.

If the real-kernel tests expose an incompatibility, add a focused protocol
fixture and adapter diagnostic. Do not introduce a Jupyter server: the selected
client already speaks directly to local kernel processes over ZeroMQ.

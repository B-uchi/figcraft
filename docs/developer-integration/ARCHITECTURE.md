# Developer integration: architecture

How FigCraft connects to a developer's local project and to MCP clients such as Claude Code. Written
for maintainers reviewing the change. The audit behind it is in
[RECONNAISSANCE.md](RECONNAISSANCE.md).

## Principles

- Local only: no account, no backend, no new HTTP service.
- One MCP server, one command registry, one control protocol. Developer Connect is a second way to
  reach them, and `--control` and `--attach` behave as they did.
- The app owns grants. An agent can't give itself access, because a grant is not a command.
- Nothing machine-specific goes in the project.
- Client-neutral: Claude Code is one row in a table of clients, and the engine knows nothing of it.

## Shape

```text
Developer's project                 Per-user folder (user-only)        FigCraft desktop
 .mcp.json ─ "figcraft-cli mcp       <platform config>/FigCraft/        ├─ Developer Connect window
              --project"             Connect/<pid>.json                 ├─ Hub: grants, links, status
 figcraft.connect.json                {port, token, pid, version}       ├─ Listener on 127.0.0.1:<free>
   project, links                            ▲                          ├─ Prefs: connected projects
                                             │ written by the app       └─ UI thread: control.rs, engine
Claude Code                                  │ read by the CLI                    ▲
   │ stdio JSON-RPC                          │                                    │ queue, per-request roots
   ▼                                         │                                    │
 figcraft-cli mcp --project ─────────────────┘                                    │
   └─ figcraft_mcp::Server ─ Attached ─ auth ─ TCP loopback ─ lines ──────────────┘
```

## Connection lifecycle

```text
App starts ── prefs.connect.enabled? ── yes ─▶ listen, write discovery file
                                         no ─▶ waits for the window's switch
Client starts figcraft-cli mcp --project
   finds the project: nearest folder upwards with figcraft.connect.json, else the working folder
   initialize ─▶ reads discovery files, newest first
       each: TCP connect ─ auth {token, project, client}
         ok  ─▶ project connection: confined to the project, shows in the window
         no  ─▶ next file
       none ─▶ server keeps running; each call says what to do, and tries again
Idle         ─▶ connection stays open (no idle timeout once authenticated)
Client quits ─▶ socket closes, the link leaves the window, the app carries on
Project disconnected in the window ─▶ link closed; further calls say so
App quits    ─▶ discovery file removed; next call: "FigCraft isn't running", nothing sent
App reopens  ─▶ new port and token; next call reads the new file and reconnects
App crashes  ─▶ file stays; its port refuses, so it's skipped, and removed at the next start
```

### Handshake

The first line on a Developer Connect socket has 5 seconds to arrive and at most 64 KiB:

```json
{"id":"auth","method":"auth","params":{"token":"<64 hex>","project":"/abs/path","client":"Claude Code"}}
{"id":"auth","ok":true,"result":{"project":{"name":"shop","assetDir":"public/figcraft"},"files":{"read":["…"],"write":["…"]},"app":{"version":"0.7.0"}}}
```

This is the first-frame `auth` PhotoCraft's control channel ships, with `project` and `client`
added. Anything else ends the connection, so an HTTP request from a web page stops at its first
line, as on `--control`. A later `auth` answers `already authenticated`. After it the socket speaks the ordinary control protocol, with three differences:

| Difference | Why |
|---|---|
| File access is confined to the project on every request | One app can serve several projects, and the person at the window stays unconfined |
| A method allowlist applies | `ui.input`, `ui.menu.invoke`, `ui.set`, `window.new`, `window.focus` and `app.quit` could operate the app's own windows |
| `ui.screenshot` refuses `path`, and `agent` is always set | The file would be written later, outside the request's scope; `agent` keeps paths out of `doc.list` |

The allowlist is what the MCP tools use: `engine.execute`, `engine.commands`, `document.inspect`,
`node.get`, `ui.inspect`, `ui.pointer`, `ui.key`, `ui.type`, `ui.render`, `ui.screenshot`,
`window.list`, `events.poll`, plus `connect.info`.

A confined request also reaches no network, through the existing `network_refused()`, so an agent
can't publish a document from a project connection.

## Why not reuse `--control` as it is

| Option | Result |
|---|---|
| Start `--control` from the window on a fixed port | Unauthenticated, collides with sibling apps, and the port ends up in client configuration |
| Require a token on `--control` | Breaks existing scripts and tests |
| **A second listener sharing the queue and protocol** | Chosen: additive, and scripts keep working |
| HTTP MCP transport in the app | A new service, and the brief prefers stdio |

## Relation to control-channel authentication

Issue #480 and PRs #500 and #1473 propose authenticating `--control`. Maintainers are holding them
for a suite-wide decision, because they change a protocol all the Craft apps share. Developer Connect
doesn't depend on that decision and doesn't pre-empt it: `--control` stays exactly as it is, and the
token applies only to a new, opt-in listener.

It takes the review of those PRs into account: the listener binds before the discovery file is
written, a failed write stops it (fail closed), the token is new each run and compared in constant
time, the first line is bounded in size and in total time, and the folder comes from craft-settings.
If the suite settles on a token file, this listener and `--attach` can adopt it, and the discovery
file is the only piece that would change.

## Project configuration

| What | Where | Committed |
|---|---|---|
| The MCP server entry | `.mcp.json` | Yes |
| Project name, asset folder, code mappings | `figcraft.connect.json` | Yes |
| Which projects this computer granted | App preferences | No |
| Port, token, process id | Discovery file | No |

```json
{
  "version": 1,
  "project": {"name": "shop", "assetDir": "public/figcraft"},
  "links": []
}
```

`project` is optional and additive: `codeConnect.import` reads only `version` and `links`, and
`codeConnect.export {path}` now keeps the keys it doesn't own.

`.mcp.json` entry:

```json
{"mcpServers": {"figcraft": {"command": "figcraft-cli", "args": ["mcp", "--project"]}}}
```

The merge:

- parses first, and refuses invalid JSON or a `mcpServers` that isn't an object, leaving the file alone;
- splices the original text, so other servers, key order and formatting stay byte for byte;
- verifies by parsing the result, and writes atomically;
- does nothing when an entry is already ours and runs here, so a teammate's committed file doesn't
  churn;
- warns about another server that runs `figcraft-cli`.

When `figcraft-cli` isn't on `PATH`, the entry names the absolute path beside the app (or the
AppImage with `cli`), so setup still works, and the window warns that this file is then specific to
the computer.

## Grants

A grant is a project folder chosen in the system's folder picker. It lives in `prefs.connect`, and
no command changes it. A grant gives a connection, for that folder only:

- reads and writes inside the folder;
- symlinks and `..` resolved before comparison, by the existing `file_access` code;
- nothing else on the disk: `save_file` on a design stored outside the project is refused.

Setup itself runs confined to the folder, so a `.mcp.json` that is a link out of the project is
refused.

An app started with `--automation-read-root` or `--automation-write-root` doesn't offer Developer
Connect. A nested scope replaces the outer one, so a grant could otherwise widen those flags.

## What changes in the source

| Area | File | Change |
|---|---|---|
| Engine | `crates/engine/src/dev/project_connect.rs` (new) | Shared names, discovery format, `.mcp.json` merge, project settings, CLI lookup |
| Engine | `crates/engine/src/dev/code_connect.rs` | `export` keeps unknown keys |
| Engine | `crates/engine/src/prefs.rs` | `connect: ConnectPrefs` |
| Engine | `crates/engine/src/cmd/view.rs` | `developer.connect` opens the window |
| MCP | `crates/mcp/src/connect.rs` (new) | Discovery and handshake |
| MCP | `crates/mcp/src/backend.rs` | `Attached::project`, reconnection through discovery |
| MCP | `crates/mcp/src/tools.rs`, `server.rs` | `connection_status` tool, instructions |
| UI | `crates/ui-egui/src/connect.rs` (new) | `Hub`: grants, links, status; no egui |
| UI | `crates/ui-egui/src/dialogs/developer_connect.rs` (new) | The window |
| UI | `crates/ui-egui/src/control.rs` | `ControlRequest.roots`, applied at both call sites |
| UI | `crates/ui-egui/src/services.rs`, `app/commands.rs`, `toolbar.rs`, `menus` | Service hooks, dialog, menu items |
| App | `apps/figcraft/src/connect_server.rs` (new) | Listener, handshake, discovery file |
| App | `apps/figcraft/src/control_server.rs` | Serving loop takes an optional scope |
| App | `apps/figcraft/src/main.rs`, `services.rs` | Wiring, resume at launch, shutdown |
| CLI | `apps/figcraft-cli/src/args.rs`, `main.rs`, `files.rs` | `mcp --project`, `connect setup`, `connect status` |

The document model and format are unchanged. No new crate. `getrandom`, already in `Cargo.lock`,
becomes a direct dependency of the app for tokens.

## Conflicts with existing behaviour

| Topic | Resolution |
|---|---|
| "Everything is a command" | Grants deliberately aren't, as with font downloads, which only the user can allow. Everything an agent uses remains a command. Recorded here for maintainers |
| Commit style in the brief | The repository's `M8: …` titles are used |
| `codeConnect.export {path}` overwrote the file | Now merges into an existing file; the output for `links` is unchanged |
| Control idle timeout of 60 s | Authenticated connections have none, so status reflects real clients; closing is by the OS or the Hub |
| Web build | No sockets: the command explains that Developer Connect needs the desktop app |

## For maintainers to approve

Changes beyond ordinary feature code:

| Change | Why it needs a decision |
|---|---|
| A second listener and a discovery file holding a token | A new local trust boundary, see [SECURITY.md](SECURITY.md) |
| Grants that are deliberately not commands | An exception to "everything is a command" |
| `codeConnect.export {path}` keeps other keys | A small behaviour change to an existing command |
| `project` key in `figcraft.connect.json` | The file format grows, optionally |
| `ControlRequest.roots` | A public struct gains a field |
| `getrandom` as a direct dependency of the app | Already in the tree through `collab-net` |
| `figcraft-cli` is run with `mcp --help` during setup | Setup starts a program found on `PATH`, to check it has `--project` |

## Design context

`inspect.designContext` is one read-only command in `crates/engine/src/dev/design_context.rs`, so the
CLI, the control channel and MCP share it. It adds no document state. Tree layers reuse `get_node`'s
serialisation, then separate it:

| Part | Rule |
|---|---|
| `explicit` | What `get_node` reports, less what follows |
| `derived` | `absoluteBounds`, `rotation`, `gridCell`; `x` and `y` in an auto layout flow; a width or height that Hug, Fill or auto-sizing text decides |
| `unsupported` | A short table of what CSS can't say directly. Only well-known limits are listed, and the value stays in `explicit` |
| `missing` | What the file doesn't hold, such as breakpoints, or a variable it no longer has |

Limits keep results usable: 2,000 layers and depth 32 by default, and the MCP tool halves the layer
count until the result fits the client. A connected instance stops at its code.

## Later phases

| Phase | Design direction |
|---|---|
| 3 | Asset listing and original-bytes export into `assetDir`, named by content hash |
| 4 | Optional link fields, `component_mapping` reads, imports applied on connection |
| 5 | A documented annotation category for responsive instructions; frames grouped only when the designer says so |
| 6 | Reference renders at explicit sizes, compared by the agent with the developer's browser tooling |

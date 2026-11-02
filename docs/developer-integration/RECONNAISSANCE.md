# Developer integration: reconnaissance

Audit of the FigCraft source at `f9b56936` (2026-11-02), made before any Developer Connect code
existed. It records what the repository already does, what is missing and what that means for the
design in [ARCHITECTURE.md](ARCHITECTURE.md). Statements come from the source unless marked
*measured* (observed on the running app, see [Measured behaviour](#measured-behaviour)).

## 1. Summary

FigCraft already has most of the machinery the feature needs. A complete MCP server, an attached
mode, file confinement, Dev Mode inspection, code generation, annotations and Code Connect all
exist and are tested. Five gaps stand between them and the intended workflow:

1. **The connection can only be made at launch.** `figcraft --control PORT` is the only way to open
   the channel, so an app the user opened normally can't be reached.
2. **The channel has no authentication.** Any local process that reaches the port has every method,
   including `app.quit`, `ui.input` and `ui.menu.invoke`.
3. **Confinement is process-wide and fixed at launch.** One project can't get its own folders, and
   the app's window would be confined along with the agent.
4. **Setup is manual.** The port is machine state, yet it has to be written into the client's
   configuration, and nothing merges `.mcp.json`.
5. **Handoff data is scattered.** No tool returns "the selected frame, with its tokens, components
   and assets" in one read, and no command returns an image's original bytes.

## 2. MCP architecture

```text
Claude Code ──stdio JSON-RPC──▶ figcraft-cli mcp ──▶ figcraft_mcp::Server
                                                       ├─ tools.rs      (19 tools)
                                                       ├─ resources.rs  (figcraft://…)
                                                       ├─ prompts.rs    (6 workflows)
                                                       └─ dyn Backend
                                                            ├─ Headless  in-process Session
                                                            └─ Attached  TCP to the app ─▶ control.rs
```

| Area | Where | Facts |
|---|---|---|
| Transport | `crates/mcp/src/server.rs` | Newline-delimited JSON-RPC on stdio only; protocol `2025-06-18`, also `2025-03-26` and `2024-11-05`; 32 MiB line limit |
| Tool registration | `crates/mcp/src/tools.rs` | `tools()` and `TOOL_NAMES` must agree; `tests/it/protocol.rs` checks the order, descriptions, schemas and `readOnlyHint` |
| Command registry | `crates/engine/src/cmd/*`, `spec!` | One registry. MCP `run_command` and `batch` reach all of it |
| Dispatch | `Server::request` → `call_tool` → `Backend::call` | Every tool is a control-channel method call, so Headless and Attached share behaviour |
| Errors | `ToolResult` | A tool failure is `isError: true`, a protocol failure is a JSON-RPC error, and a panic fails one request only |
| Document access | `document`, on tools acting on a file | Names an open file without switching tabs |
| Resources | `crates/mcp/src/resources.rs` | `figcraft://document`, `commands`, `comments`, `node/{id}`, `command/{id}` |
| Prompts | `crates/mcp/src/prompts.rs` | `mobile_screen`, `design_system`, `landing_page`, `prototype_flow`, `fix_comments`, `watch_comments` |
| Agent identity | `Server::agent`, `AsAgent` | `clientInfo` becomes `agent` on `engine.execute`; the app shows presence and signs comments with it |

### Tool inventory

| Tool | Control methods | Read-only hint |
|---|---|---|
| `list_commands` | `engine.commands` | yes |
| `run_command`, `batch` | `engine.execute` (`command.batch`) | no |
| `create_node`, `set_properties` | `node.create`, `node.set`, `node.get`, `document.inspect` | no |
| `inspect_document`, `get_node` | `document.inspect`, `node.get` | yes |
| `select` | `select.set`, `select.none`, `document.inspect` | no |
| `export` | `file.export` | no |
| `screenshot` | `ui.render`, or `ui.screenshot` for a window | yes |
| `open_file`, `save_file` | `file.open`, `file.save` | no |
| `undo`, `redo` | `edit.undo`, `edit.redo` | no |
| `pointer`, `key`, `type_text` | `tool.set`, `ui.pointer`, `ui.key`, `ui.type`, `text.edit.*` | no |
| `inspect_ui` | `ui.inspect` | yes |
| `wait_for_changes` | `events.poll` | yes |

The names the brief lists, checked against the source:

| Name | Status |
|---|---|
| `inspect_document`, `get_node`, `export`, `screenshot` | MCP tools |
| `inspect.properties`, `inspect.code`, `inspect.componentCode`, `inspect.assets` | Engine commands, reachable only through `run_command` (not marked read-only) |
| `codeConnect.set`, `.remove`, `.export`, `.import` | Engine commands, reachable only through `run_command` |
| `variable.list`, `style.list`, `annotation.list`, `component.properties` | Engine commands, reachable only through `run_command` |

### Missing capabilities

- A way to connect without knowing a port, and a server that keeps running when the app isn't up.
- A selection-scoped read (`document.inspect` gives ids only).
- Dedicated read-only tools for handoff, so an agent doesn't use the mutating `run_command` to read.
- MCP asset handoff with stable names and original bytes.

## 3. CLI and desktop attachment

| Topic | Today |
|---|---|
| `figcraft-cli mcp` | Headless session with one untitled document |
| `figcraft-cli mcp --attach <port\|host:port>` | Connects once at startup and **exits if nothing is listening** |
| `--control <port>`, `FIGCRAFT_CONTROL_PORT` | Binds `127.0.0.1:port`. On failure it logs and the app carries on without a channel; the user sees nothing |
| Port selection | Fixed by the caller, no automatic choice. `bind` retries while a restarting app holds the port |
| Authentication | None. A line that isn't JSON closes the connection, which stops HTTP requests a web page sends |
| Connections | 8 at most; a ninth is dropped without a reply. Reads and writes time out after 60 s idle |
| Queue | 8 requests per queue; a full one answers `control request queue is full` |
| Desktop closes | Workers end with the process. Attached calls fail; the next call reconnects |
| Client disconnects | Worker exits on EOF. The app doesn't record it |
| Several agents | Serialised on the UI thread with no ownership, and all act on the same selection and document |
| Status | Nothing in the UI says the channel is on. Agent presence shows for 8 s after a command |

```text
figcraft --control P ─ bind ─ listen ◀─ connect ─ figcraft-cli mcp --attach P (exits if refused)
        │                       │  ◀─ line ─▶ queue ─▶ UI thread ─▶ reply
        │                       │  60 s idle: app closes the socket
        │                       │             Attached notices (stale) and reconnects, nothing replayed
        └─ app exits ───────────┘             next call: "not reachable", still serving
```

### Reliability assessment

The attached transport itself is good. Replay safety, idle reconnection, reply limits and timeouts
are all handled and tested. It is **not ready for this workflow**, for these reasons:

| # | Issue | Effect |
|---|---|---|
| R1 | Channel only starts at launch | A user who opened FigCraft normally can't connect at all |
| R2 | The port is written into client configuration | Machine state in a committed file, and collisions between Craft apps |
| R3 | `--attach` exits when the app is closed | The client marks the server failed, and it stays failed after the app opens |
| R4 | A bind failure is silent | The app looks fine and agents get `connection refused` |
| R5 | A ninth connection is dropped without a reply | The client sees an unexplained EOF |
| R6 | No connection state in the app | Status can't be shown; idle timeouts would look like disconnects |

## 4. Security boundaries

| Boundary | State |
|---|---|
| Network exposure | Loopback only, opt-in, and `--attach host:port` can name another host but the app listens on loopback |
| Web pages | Protected: non-JSON lines end the connection |
| Other local users and processes | **Not protected.** Anyone who can reach loopback has all methods |
| File access | `file_access` is the single engine gateway, enforced by clippy, covered by a sweep over every command. Symlinks and `..` are resolved before comparison, and writes are atomic |
| Network from agents | `network_refused()` blocks publish, live sessions, team libraries and plugin directories while confined |
| Path privacy | `doc.list` hides paths from agents (#1117). `document.inspect` still returns `path` |
| Self-widening | Attached agents have `ui.input`, `ui.menu.invoke` and `ui.set`, so they could operate the app's own windows |

Confinement is thread-scoped (`file_access::confined`). A nested scope **replaces** the outer one,
it doesn't intersect it. `windows::guard` confines all work to `app.automation_roots`. Two call
sites run control methods (`control.rs` `background` and `drain`), and `ui.screenshot {path}` writes
later, from the capture path, so per-request confinement has to treat it separately.

## 5. Code Connect

Source: `crates/engine/src/dev/code_connect.rs`, Dev Mode panel `crates/ui-egui/src/panels/dev_mode.rs`,
CLI `figcraft-cli connect publish`.

| Topic | State |
|---|---|
| Model | `Node::code_links: Vec<CodeLink {language, import_path, name, template}>` on the component or set |
| Persistence | **In the document** (saved, synced, undoable), and portable through `figcraft.connect.json` |
| File | `{version: 1, links: [{component, language, importPath, name, template}]}`, sorted |
| Identity | By component **name**. An import links every component with that name |
| Commands | `codeConnect.set`, `remove`, `export`, `import` |
| Frameworks | React, Leptos, Vue, SwiftUI, Compose, ArkUI, Flutter |
| Property mapping | Template placeholders: `{prop}` and `{prop ? "a" : "b"}`, never executed |
| Consumers | `inspect.code`, `inspect.componentCode`, Dev Mode: instances render as their connected snippet, with import lines |
| MCP | `get_node` reports `codeLinks`, and `inspect.code` renders them. No dedicated tool |

Missing for a complete experience:

- The file holds mappings only: no project identity, asset preferences or document references.
- `codeConnect.export {path}` rewrites the whole file, so any added key would be lost.
- No source file path, only an import path, and no check that the code component exists.
- No explicit variant or default-value mapping; everything is expressed in the template.
- A file has to be imported by hand. Opening a design doesn't apply a connected project's mappings.

**Proposal:** keep `figcraft.connect.json` and `CodeLink`. Add an optional top-level `project` key
(older readers ignore it, because `import` reads only `version` and `links`), make `export` keep
keys it doesn't own, and add optional per-link fields in Phase 4. No second mapping system.

## 6. Design data availability

Exact = stored and returned unchanged. Derived = computed. Absent = the model has nothing.

| Concept | Storage | Read through | Status |
|---|---|---|---|
| Document, pages | `doc/src/lib.rs`, `node.rs` | `document.inspect` | Exact; styles as a summary, variables and images as counts |
| Frames, groups, sections | `NodeKind` | `type`, `sectionContentsHidden` | Exact |
| Position, size | Node | `x`, `y`, `width`, `height` exact; `absoluteBounds` derived | Exact and derived |
| Rotation | `transform` | `rotation` | Derived; scale, skew and flip aren't reported |
| Constraints | `props.rs` | `constraints` | Exact, only for children of plain frames and absolute children |
| Auto layout, grid | `props.rs` | `layout`, `gridCell` (derived) | Exact |
| Sizing, min/max | `props.rs` | `sizing`, `minWidth`… | Exact; absent from `inspect.properties` |
| Fills, strokes | Node | `fills`, `strokes` | Exact; stroke caps not reported |
| Opacity, visibility, radius, clip | Node | Same keys | Exact; omitted at default, `clip` on frame-like layers |
| Effects | `props.rs` | `effects` | Exact |
| Family, style, size | `text.rs` | `fontFamily`, `fontStyle`, `fontSize` | Exact; weight is the style's name |
| Line height, spacing | `text.rs` | Label strings | Exact, as strings such as `120%` |
| Align, content, decoration | `text.rs` | `textAlign`, `characters`, `textDecoration` | Exact |
| Styled runs | `text.rs` | `styledRuns`, `textStyleRanges` | Exact |
| Vertical align, case, paragraph spacing, indent, OpenType features | `text.rs` | Not on `get_node` (`paragraph_spacing` in `inspect.properties`, case in CSS) | **Stored, not read** |
| Component, properties | `PropDef` | `componentPropertyDefinitions`, `component.list` | Exact |
| Variants | `K=V` child names | `component.properties`, `inspect.componentCode` | Derived |
| Instance | Node | `mainComponent`, `componentProperties`, `overrides` | Exact |
| Nested exposure, swap source, property bindings | `exposed`, `source`, `prop_refs` | Used by codegen only | **Stored, not read** |
| Slots | Node | `slot` | Derived |
| Variables | `styles.rs` | `variable.list`; `boundVariables` as ids | Exact, join needed |
| Resolved value per mode | `resolve` | No command | **Derived, no read** |
| Layer variable modes | `var_modes` | Write-only | **Stored, not read** |
| Styles | `styles.rs` | `style.list`; `styles` as ids | Exact, join needed |
| Annotations | Node, categories on the document | `annotations`, `annotation.list` | Exact: free text, pinned properties, category. No author or date |
| Handoff status | Node | `devStatus`, `devChanged`, `devNote` | Exact |
| Dev resources, measurements | Node | `devResources`, `measurements` | Exact; distances derived |
| Interactions, flows | Node, page | `interactions`, `flows` | Exact |
| Device per page | `proto.rs` | Not on `document.inspect` | Presentation setting only |
| Frame presets | `cmd/create.rs` | `frame.presets` | Creation only, not kept on the frame |
| **Breakpoints, alternate frames, responsive variants, hide-below-width** | None | None | **Absent** |
| Selection | `DocState.selection` | `document.inspect` ids | Exact, ids only, per open document |

Inspection limits already help with context size. `get_node` has an 8 MiB limit, `maxBytes`, depth,
`fields`, `outline`, and paging with a `truncated` marker.

**Responsive consequence:** nothing in the model states a breakpoint. Constraints, sizing and auto
layout are the only explicit information. Phase 5 needs explicit designer input, and annotations
are the natural carrier because they are free-form, categorised, saved and already readable.

## 7. Asset pipeline

| Topic | State |
|---|---|
| Storage | `Document.images: map<key, ImageData{bytes, width, height}>`, the **original encoded bytes** unchanged |
| Keys | SHA-256 hex normally, but FNV for SVG imports and a placeholder for REST imports, so not always a content hash |
| Format | Not stored; sniffing is private to `crates/svg` |
| `.figcraft` | Zip with `images/<key>`, stored once per key |
| Original bytes out | **No command.** `file.export`, `file.exportLayers` and `{assets: true}` all re-render; image assets become PNG |
| SVG | `file.exportSvg`, `file.export`, MCP `export`. Embeds original PNG, JPEG, GIF and WebP as base64; some effects become PNG; text is outlined by default |
| `inspect.assets` | `{assets: [{id, name, kind, exports}]}`, kinds `export`, `icon`, `image`; no hash; caps of 200 assets; hidden layers skipped |
| `file.exportLayers` names | From layer names and order; `/` makes folders; unsafe characters become `_`; collisions get ` 2` within one call; existing files are replaced |
| Fonts | Family and style strings only. No font export exists (PDF embeds subsets). `text.missingFonts` lists what's missing |
| Hash exposure | Non-solid paint JSON, `image.place` |

**Handoff design (Phase 3):** a read-only asset list for a subtree and an export command that writes
original bytes to `<assetDir>/<content-hash>.<sniffed ext>`, so names are deterministic and repeated
exports are no-ops. Vectors go through the existing SVG exporter. An image is never replaced by a
render without saying so, and each entry reports whether it is original or rendered. Fonts are
reported by name only: the document holds no font bytes, and system fonts carry licences FigCraft
can't check, so FigCraft doesn't redistribute them.

## 8. Tests and conventions

| Layer | Where | Technique |
|---|---|---|
| Unit | Sibling `*_tests.rs` | Plain `#[test]` |
| MCP protocol | `crates/mcp/tests/it/*` | In-process client through `Server::handle_line` |
| Attached | `crates/mcp/tests/it/attached.rs` | `fake_app()`: a TCP listener answering through `Headless` |
| CLI | `apps/figcraft-cli/tests/it/*` | Runs the real binary |
| Control transport | `apps/figcraft/src/control_server_tests.rs` | Loopback sockets, fake queue |
| UI | `crates/ui-egui/src/*_tests.rs` | `FigApp` in-process |
| Real app | None | No automated test launches the desktop app |

`cargo xtask ci` runs, stopping at the first failure: fmt, clippy `-D warnings`, tests and doctests,
the i18n report, layers, assets, docs (repeated rows), then the wasm check and plugin checks.

What a change must satisfy:

| Change | Gates |
|---|---|
| Engine command | Junk-parameter sweep, roots sweep, `every_doc_item_names_a_parameter`, wasm |
| MCP tool | `TOOL_NAMES` order, description over 40 chars, object schema, `readOnlyHint` |
| CLI subcommand | Clap tests, generated docs |
| UI string | `every_key_in_the_code_is_in_the_english_catalog_and_no_other`; add to `locales/en.toml` |
| Engine file I/O | Only through `file_access` (`crates/engine/clippy.toml`) |

Commit titles use a milestone or area prefix and a plain behaviour sentence
(`M8: …`, `docs: …`), so the repository's convention replaces the brief's `feat(mcp):` examples.

**Test strategy:** extend each existing layer rather than build a new harness: `Server::handle_line`
for protocol, loopback sockets for the handshake, `FigApp` for the UI and the roots tests for
confinement, plus a scripted procedure against the real app and a real client, because no automated
test launches the desktop app.

## 9. Proposed changes

Details and file lists are in [ARCHITECTURE.md](ARCHITECTURE.md).

| Phase | Change |
|---|---|
| 1 | In-app, token-protected control listener on a free port; per-project grants; a discovery file; `figcraft-cli mcp --project`; idempotent `.mcp.json` setup; Developer Connect window |
| 2 | Selection-aware, token-joined design context tool |
| 3 | Original-bytes asset export confined to the project |
| 4 | `project` and per-link additions to `figcraft.connect.json`, dedicated MCP reads |
| 5 | Responsive information through a defined annotation convention |
| 6 | Reference render at explicit dimensions, browser driven by the developer's own tools |

## 10. Risks

| Risk | Mitigation |
|---|---|
| A same-user process can read the discovery file | Stated in [SECURITY.md](SECURITY.md): the token stops other users and web pages, not your own programs |
| An agent widens its own access | Grants exist only in the window, never as a command; the project connection has a method allowlist |
| A deferred write escapes request confinement | `ui.screenshot {path}` is refused on project connections |
| Long-lived connections use up the 8 slots | Project connections have their own listener and count, and an over-limit client gets a reason |
| Reordering a user's `.mcp.json` | Edits splice the original text, so unrelated bytes stay the same |
| Stale discovery files after a crash | Tokens are per run, so an old one opens nothing; unreachable entries are skipped and removed |
| Upstream may prefer other naming or scope | Changes are additive, and `--control` and `--attach` are untouched |

## 11. Implementation sequence

Phase 1 then 2, 3, 4, 5, 6, each with its own tests and report. Phase 1 order: shared project
configuration, then the authenticated listener and per-request confinement, then the CLI and
`Attached`, then the window, then documentation.

The phases are implemented as described in [ARCHITECTURE.md](ARCHITECTURE.md). This report records the
repository as it was before them.

## 12. Questions for maintainers

1. Is a per-run token in a user-only discovery file acceptable as the local trust boundary?
2. Should project connections stay read-and-edit, or should the app offer a read-only grant?
3. Should the `path` in `document.inspect` be hidden from agents, as `doc.list` already does?
4. Is a defined annotation category the preferred way to carry responsive instructions in Phase 5?
5. Should `.mcp.json` setup also cover other clients' files, once a second client is requested?

## Measured behaviour

Observed on Linux (Ubuntu 24.04, X11/Wayland session) with the debug build of the app, before and
while the feature was added. `--control` and `--attach` were not changed.

| Probe | Result |
|---|---|
| `figcraft-cli mcp --attach PORT` with no app | Exits with 1: `can't reach the FigCraft app … Connection refused` |
| Any local client sending `{"method":"app.quit"}` to `--control` | The app quits. No credential is asked for |
| Attached call on a first run, no saved preferences | `no open document`: the app opens on Home, so a script must `file.new` first |
| Installed `figcraft-cli` 0.5.1 | Has no `mcp --project`, so a CLI found on `PATH` can't be assumed current |

**Toolchain:** the workspace declares `rust-version = "1.95"`, but `crates/pathops/src/budget.rs` uses
`isolate_lowest_one`, which Rust 1.95 rejects as unstable. Rust 1.99.1 builds it. The audit used
1.99.1 and left `rust-version` alone, because this is unrelated to the feature; maintainers should
raise it.

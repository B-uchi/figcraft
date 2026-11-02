# Testing Developer Connect

For maintainers checking the integration. The automated tests cover each layer. No automated test
launches the desktop app, so the procedure below covers the rest by hand.

## Automated tests

| What | Where |
|---|---|
| `.mcp.json` merge, project setup, CLI lookup | `crates/engine/src/project_connect_tests.rs` |
| Design context | `crates/engine/src/design_context_tests.rs` |
| Asset handoff | `crates/engine/src/asset_handoff_tests.rs` |
| Code Connect | `crates/engine/src/code_connect_tests.rs` |
| Responsive context | `crates/engine/src/responsive_tests.rs` |
| Visual comparison | `crates/engine/src/visual_compare_tests.rs` |
| Handshake, confinement, allowlist, discovery file | `apps/figcraft/src/connect_server_tests.rs` |
| Grants, connecting, disconnecting | `crates/ui-egui/src/connect_tests.rs` |
| MCP server against a fake app: lifecycle, assets, mappings | `crates/mcp/tests/it/project.rs` |
| The CLI binary: setup, `mcp --project`, status | `apps/figcraft-cli/tests/it/cli.rs` |

```sh
cargo test -p figcraft-engine -p figcraft-mcp -p figcraft -p figcraft-cli -p figcraft-ui-egui
```

## End-to-end procedure

Use a synthetic document and a throwaway project. `FIGCRAFT_CONFIG_DIR` keeps it apart from your
settings.

```sh
cargo build -p figcraft -p figcraft-cli
export PATH="$PWD/target/debug:$PATH" FIGCRAFT_CONFIG_DIR=/tmp/figcraft-e2e
mkdir -p /tmp/shop && cd /tmp/shop && git init
figcraft
```

| # | Do | Expect |
|---|---|---|
| 1 | In the app, make a frame with a text layer, a rectangle and an image. Select the frame | |
| 2 | **?** ▸ **Developer Connect…** ▸ **Connect a project…**, choose `/tmp/shop` | `.mcp.json` and `figcraft.connect.json` appear. Connecting again changes neither |
| 3 | Run `claude` in `/tmp/shop` and approve `figcraft` | **Agents connected** names Claude Code |
| 4 | Ask for `connection_status` | The document, 1 layer selected, and the project's folder |
| 5 | Ask for `design_context` | The frame only, with real measurements, and `missing` naming responsive behaviour |
| 6 | Ask to run `export_assets` | The image in `public/figcraft/`, byte-identical to the original, `source: original` |
| 7 | Run it again | `written: 0` |
| 8 | Ask to export to `/tmp/elsewhere` | Refused: `automation path rejected` |
| 9 | Make a component, connect it in the Inspect tab, then use **Export** in Developer Connect | The mapping is in `figcraft.connect.json` |
| 10 | Ask for `component_mappings` | The import, name and snippet |
| 11 | In **Screen sizes**, mark the frame **Desktop** and add a rule; ask for `design_context` | `responsive.explicit` is true, with the rule |
| 12 | Build the frame as a page, capture it at the frame's size, ask for `compare_design` | Differences by layer; none once the page matches |
| 13 | Quit FigCraft and call any tool | `FigCraft isn't running…`, and the server keeps running |
| 14 | Open FigCraft and call a tool | It works, with no restart of Claude |
| 15 | **Disconnect** the project | The next call says it was disconnected. The project's files stay |

## Security checks

```sh
port=$(python3 -c 'import glob,json;print(json.load(open(glob.glob("/tmp/figcraft-e2e/Connect/*.json")[0]))["port"])')
curl --max-time 2 "http://127.0.0.1:$port/"             # an error line, then the connection ends
echo '{"method":"app.quit"}' | nc -q1 127.0.0.1 "$port"  # not authorized, and the app stays open
ls -l /tmp/figcraft-e2e/Connect                          # -rw------- on Unix
```

## Status

Run on Linux with the real app, Claude Code and Chrome. macOS, Windows and Flatpak haven't been run.

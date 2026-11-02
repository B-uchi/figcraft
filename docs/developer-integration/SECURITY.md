# Developer Connect security

What Developer Connect protects, how, and where its limits are.

## Local only

- No account, backend or telemetry. Design data moves between FigCraft and the agent's process on
  your computer.
- The listener binds `127.0.0.1` on a free port chosen by the system.
- What the agent's client then does with what it reads is that client's business. A cloud-backed
  model receives the design data the agent reads, as it receives your code.

## Who can connect

| Caller | Outcome |
|---|---|
| A web page | Refused: its request's first line isn't a valid `auth`, so the connection ends |
| Another user on this computer | Refused: the token is in a file only you can read |
| A previous run's token | Refused: every run makes a new 256-bit token |
| Your own programs | **Allowed if they read the file.** See below |
| Another computer | Not reachable: loopback only |

The discovery file is `<settings folder>/Connect/<pid>.json`, mode `0600` in a `0700` folder on Unix.
On Windows it is protected by the per-user permissions of `%APPDATA%`, with no ACL of its own. A
portable copy keeps it in its `config` folder, with whatever permissions that folder has.

**Limit:** anything running as you can read the token, as it can read your other files. The token
separates you from other users and from web pages. It doesn't defend against software you already
run with your own permissions.

## What a project connection can do

Every request on a project connection runs with these rules, in `apps/figcraft/src/control_server.rs`:

- **Files:** only inside the connected folder, through the same `file_access` gateway and sweep tests
  as the automation roots. Links and `..` are resolved before comparing, and a link out of the
  folder is refused.
- **Network:** none. Publishing, live sessions, team libraries and plugin directories are refused.
- **Methods:** an allowlist. `ui.input`, `ui.menu.invoke`, `ui.set`, `window.new`, `window.focus` and
  `app.quit` aren't on it, so an agent can't operate the app's own windows.
- **Identity:** always an agent, so `doc.list` hides paths from it.
- **Screenshots:** returned as data. `ui.screenshot {path}` is refused, because that file is written
  after the request's confinement ends.

`export_assets`, `compare_design` and `sync_mappings` go through the same gateway, so they read and
write only inside the project.

An agent still has full design access. It can change or delete anything in the open documents,
through ordinary undoable commands, and it can read their names and paths through `document.inspect`.

## Grants

- Only the Developer Connect window grants a folder. No command, MCP tool or control method does, so
  an agent can't widen its own access.
- Choosing a folder gives read and write access to that folder for that project's connections, not
  to the whole disk.
- Disconnecting takes effect at once: open connections are closed.
- `figcraft-cli connect setup` writes project files and grants nothing.

## Configuration files

- `.mcp.json` is parsed before anything is written. A file that isn't valid is left untouched.
- Edits splice the text and are verified, then written atomically.
- Setup is confined to the project, so a `.mcp.json` linking out of the folder is refused.
- Asset folders must be relative paths going down from the project.

`.mcp.json` runs commands. Review changes to it in repositories you don't trust; Claude Code asks
for approval before starting a project's server.

## Failure handling

| Event | Outcome |
|---|---|
| Client disconnects | The connection ends and the app carries on |
| Client is killed | The system closes the socket, same outcome |
| App quits | The discovery file is removed; the client's next call says the app isn't running |
| App crashes | The file stays, its token is dead, and the next start removes it |
| A request is cut off | Never replayed: the agent is told the operation may have completed |
| Slow or silent connection | Has 5 seconds in all to authenticate |

## `--control`

`figcraft --control PORT` is unchanged and has **no** authentication: any local process that reaches
the port has every method. It remains opt-in for scripts and tests. The window says when it's open.

## Reporting

Report suspected vulnerabilities privately to the maintainers, not in a public issue.

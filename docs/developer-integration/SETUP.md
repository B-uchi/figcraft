# Developer Connect setup

Connect a design open in FigCraft to an AI coding agent working in your local project. There is no
account and FigCraft uploads nothing. Your agent's model does see what the agent reads, as it sees
your code, so a cloud-hosted model receives that design data.

Supported MCP clients: Claude Code is set up automatically. Other stdio MCP clients work with the
manual steps below.

## What you need

- The FigCraft desktop app (the browser build can't listen for local agents).
- `figcraft-cli` from the same release as the app. Having it on `PATH` keeps `.mcp.json` free of
  computer-specific paths. An older CLI on `PATH`, without `mcp --project`, is passed over for the
  one beside the app, and `.mcp.json` then names that path.
- Claude Code in a terminal or in VS Code.

## Connect a project

1. In FigCraft, open **Developer Connect** from the **?** button at the bottom right of the canvas,
   or search for it in the Actions palette.
2. Click **Connect a project…** and choose your project folder.
3. In the project, start Claude Code. The window shows the command, with a Copy button:

   ```sh
   cd /path/to/project && claude
   ```

4. Approve the `figcraft` server when Claude Code asks. It asks once, because the server comes from
   the project's `.mcp.json`.
5. In Developer Connect, the project's **Agents connected** row now names Claude Code.

Ask the agent to run `connection_status` to see what it can reach.

## Implement a design

Select a frame in FigCraft, then ask the agent to implement it. In Claude Code the
`implement_design` prompt does this in one step: run `/mcp__figcraft__implement_design`. The agent
reads the selection with `design_context`, which returns real measurements, tokens, components and
what the design doesn't specify, as described in [the MCP guide](../mcp.md#design-context).

## What connecting does

| In | Change |
|---|---|
| `.mcp.json` | Adds or updates the `figcraft` server. Other servers, order and formatting stay as they are |
| `figcraft.connect.json` | Created when missing. An existing one keeps its mappings and gains a `project` key |
| FigCraft's preferences | Remembers the folder on this computer, and turns on listening |

Both project files are meant for version control. Neither holds a port, a token or a path, unless
`figcraft-cli` isn't on `PATH`: then `.mcp.json` names its path on this computer and the window warns
you. Connecting again changes nothing.

Connecting a folder gives agents in that project read and write access to that folder only. See
[SECURITY.md](SECURITY.md).

## Project configuration

```json
{
  "version": 1,
  "project": {"name": "shop", "assetDir": "public/figcraft"},
  "links": []
}
```

- `project.name`: the name FigCraft shows and agents see.
- `project.assetDir`: where design assets are meant to go, a path inside the project. Change it in
  the window or in the file.
- `links`: Code Connect mappings, see [CODE_CONNECT.md](CODE_CONNECT.md).

## Several projects

Connect as many as you like. Each agent's server finds its own project: the nearest folder upwards
with a `figcraft.connect.json`. Several agents can work at once, up to 8, and they share the open
document and selection.

## Without the window

`figcraft-cli connect setup [DIR]` writes the same two files. It grants nothing: connect the folder
in the window as well. `figcraft-cli connect status [DIR]` reports on a project and the app, and
exits with 1 when something is missing.

## Other MCP clients

Point the client at this command, with the project as its working folder:

```sh
figcraft-cli mcp --project
```

`--project DIR` names the folder instead. Then connect the folder in Developer Connect.

## Disconnecting

**Disconnect** ends the project's agents at once and removes the folder from this computer's list.
`.mcp.json` and `figcraft.connect.json` stay, because they belong to the project and may be
committed. Unticking **Listen for connected projects** disconnects every agent and stops listening,
also at the next start.

## Existing `--control` setups

`figcraft --control PORT` and `figcraft-cli mcp --attach PORT` work as before, for scripts and CI.
That channel has no token or per-project limits, so use Developer Connect for day-to-day work.

## Known limitations

- Only one document and selection at a time per window: agents see what the window in front shows.
- A design saved outside the project can't be saved by an agent; you save it in the app.
- The app must be open. The server waits and reconnects without restarting the client.
- With two FigCraft processes running, the one started last is tried first.
- Flatpak needs a manual step and hasn't been tested, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md#flatpak).
- An app started with `--automation-read-root` or `--automation-write-root` doesn't offer Developer Connect.

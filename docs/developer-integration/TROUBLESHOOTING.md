# Developer Connect troubleshooting

Start with `figcraft-cli connect status` in the project, and ask the agent to run `connection_status`.

| What you see | Cause | Fix |
|---|---|---|
| `FigCraft isn't running, or Developer Connect is off` | The app is closed, or listening is off | Open FigCraft and tick **Listen for connected projects**. The next call reconnects |
| `the project … isn't connected` | The folder isn't in this computer's list | **Connect a project…** in the window |
| `this project was disconnected in FigCraft` | It was disconnected while the agent ran | Connect it again |
| `figcraft-cli wasn't found` | No CLI with `mcp --project` on `PATH` or beside the app. An older installed one doesn't count | Update it, or build it with `cargo build -p figcraft-cli` |
| `.mcp.json isn't valid JSON` | The file was already broken | Fix the file: FigCraft leaves it untouched |
| `.mcp.json names a path on this computer` | `figcraft-cli` isn't on `PATH` | Put it on `PATH`, connect again, then commit |
| `.mcp.json also has a server named …` | Another entry runs `figcraft-cli` | Remove it, or the agent sees FigCraft twice |
| `automation path rejected: … outside the folders` | The agent used a path outside the project | Use a path in the project |
| `automation roots are set: commands don't reach the network` | A connected agent tried to publish or share | Do it yourself in the app |
| `` `app.quit` isn't available to a connected project `` | That method operates the app's windows | Not available to agents |
| `FigCraft has 8 agents connected` | All slots are in use | Close an agent |
| `control request queue is full` | Several agents sent work at once | Retry after a moment |
| `the operation may have completed` | The connection dropped during a call | Inspect the document before retrying |

## Claude Code

- **The server isn't listed:** run `claude` from the project folder and check `claude mcp list`.
- **`figcraft` shows as pending:** approve it when asked, or run `/mcp` in Claude Code.
- **`figcraft` shows as failed:** run `figcraft-cli mcp --project` in the project. It should wait
  silently on stdin. If the command isn't found, see `figcraft-cli wasn't found` above.

## The window

- **Developer Connect is greyed out or says it needs the desktop app:** the browser build can't listen.
- **"FigCraft was started with automation roots":** start it without those flags.
- **Control channel "Didn't open":** another program holds the `--control` port. This doesn't affect
  Developer Connect, which picks a free port.

## Flatpak

The sandboxed app keeps its settings in `~/.var/app/ai.storyteller.figcraft/config/FigCraft/`, which a
CLI on the host doesn't look in. Add this to the server's entry in `.mcp.json`, in the file on that
computer:

```json
"env": {"FIGCRAFT_CONFIG_DIR": "/home/you/.var/app/ai.storyteller.figcraft/config/FigCraft"}
```

The folder picker hands the app a portal path, and agents can reach only folders the sandbox can
see. This setup hasn't been tested yet.

## A crash left things behind

Nothing needs cleaning up. The next start removes old discovery files, and their tokens no longer work.

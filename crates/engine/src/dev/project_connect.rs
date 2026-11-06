//! Developer Connect's shared ground: what the app, the CLI and the MCP server agree on about a
//! developer's project. The MCP client's configuration (`.mcp.json`), the project's own file
//! (`figcraft.connect.json`, the Code Connect file with an optional `project` key) and the
//! discovery file a running app leaves for the CLI. See `docs/developer-integration/`.
//!
//! Nothing machine-specific goes in the project: which projects a computer connected is in the
//! preferences, the port and token in the discovery file. The configuration files belong to the
//! user, so they are edited by splicing the text: everything but the entry changed stays byte for
//! byte, which `serde_json` (sorted keys here) couldn't do.

use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::file_access::{self, Access, Roots};

/// The file the Code Connect mappings and the project's settings live in.
pub const CONNECT_FILE: &str = "figcraft.connect.json";
/// The name FigCraft's entry has in an MCP client's configuration.
pub const SERVER_NAME: &str = "figcraft";
/// Where exported assets go when the project doesn't say.
pub const DEFAULT_ASSET_DIR: &str = "public/figcraft";
/// The folder of discovery files, in the app's settings folder.
pub const INSTANCES_FOLDER: &str = "Connect";
/// The discovery file's format version.
pub const DISCOVERY_VERSION: u64 = 1;
/// Largest configuration file read (bytes).
const MAX_CONFIG: u64 = 4 * 1024 * 1024;
/// Longest asset folder (chars).
const MAX_ASSET_DIR: usize = 200;
/// How long the CLI gets to say what it can do.
#[cfg(not(target_arch = "wasm32"))]
const CLI_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
/// The MCP server's arguments, after whatever starts the CLI.
const MCP_ARGS: [&str; 2] = ["mcp", "--project"];

/// An MCP client FigCraft can configure. Everything specific to one is in this row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Client {
    pub id: &'static str,
    pub label: &'static str,
    /// Its project configuration file, relative to the project.
    pub config: &'static str,
    /// The object in that file holding the servers.
    pub servers_key: &'static str,
    /// The folder the client keeps in a project it is used in (`None`: set up in every project).
    pub folder: Option<&'static str>,
    /// Its entries say `"type": "stdio"`.
    pub typed: bool,
    /// What it expands to the project folder, passed to `--project`: it may start servers elsewhere.
    pub project_arg: Option<&'static str>,
}

/// The clients setup knows: project configuration files each documents.
pub const CLIENTS: &[Client] = &[
    Client { id: "claude-code", label: "Claude Code", config: ".mcp.json", servers_key: "mcpServers", folder: None, typed: false, project_arg: None },
    Client {
        id: "cursor",
        label: "Cursor",
        config: ".cursor/mcp.json",
        servers_key: "mcpServers",
        folder: Some(".cursor"),
        typed: false,
        project_arg: Some("${workspaceFolder}"),
    },
    Client {
        id: "vscode",
        label: "VS Code",
        config: ".vscode/mcp.json",
        servers_key: "servers",
        folder: Some(".vscode"),
        typed: true,
        project_arg: Some("${workspaceFolder}"),
    },
    Client {
        id: "gemini-cli",
        label: "Gemini CLI",
        config: ".gemini/settings.json",
        servers_key: "mcpServers",
        folder: Some(".gemini"),
        typed: false,
        project_arg: None,
    },
];

/// The client with id `id`.
pub fn client(id: &str) -> Option<&'static Client> {
    CLIENTS.iter().find(|c| c.id == id)
}

/// The clients to set `project` up for by default: those without a folder of their own, and those
/// whose folder the project has, so a project gets no files for tools nobody there uses.
pub fn suggested(project: &Path) -> Vec<&'static Client> {
    CLIENTS.iter().filter(|c| c.folder.is_none_or(|f| file_access::is_dir(project.join(f)))).collect()
}

/// What a running app leaves for the CLI in [`INSTANCES_FOLDER`], readable by its user only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub version: u64,
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub app_version: String,
}

// ------------------------------------------------------------------ starting the CLI

/// How a client starts FigCraft's MCP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpEntry {
    pub command: String,
    pub args: Vec<String>,
    /// `command` is a path on this computer, so a file naming it doesn't fit other computers.
    pub machine_specific: bool,
    /// Written as `"type": "stdio"`.
    pub typed: bool,
}

impl McpEntry {
    fn new(command: String, prefix: &[&str], machine_specific: bool) -> Self {
        Self { command, args: prefix.iter().chain(&MCP_ARGS).map(|a| (*a).to_string()).collect(), machine_specific, typed: false }
    }

    /// This entry as `client` writes it.
    pub fn for_client(&self, client: &Client) -> Self {
        let mut entry = self.clone();
        entry.args.extend(client.project_arg.map(str::to_string));
        entry.typed = client.typed;
        entry
    }

    fn json(&self) -> Value {
        let mut v = json!({"command": self.command, "args": self.args});
        if self.typed {
            v["type"] = json!("stdio");
        }
        v
    }

    /// The entry as text, its lines indented by `pad`.
    fn text(&self, pad: &str) -> String {
        let quoted = |s: &str| Value::from(s).to_string();
        let args: Vec<String> = self.args.iter().map(|a| quoted(a)).collect();
        let typed = if self.typed { format!("\n{pad}  \"type\": \"stdio\",") } else { String::new() };
        format!("{{{typed}\n{pad}  \"command\": {},\n{pad}  \"args\": [{}]\n{pad}}}", quoted(&self.command), args.join(", "))
    }
}

/// `figcraft-cli` with this platform's ending.
fn cli_name() -> String {
    format!("figcraft-cli{}", std::env::consts::EXE_SUFFIX)
}

/// Whether `command` starts here: a path that exists, or a bare name in `path_var`.
fn runnable(command: &str, path_var: Option<&std::ffi::OsStr>) -> bool {
    if Path::new(command).components().count() > 1 {
        return file_access::exists(command);
    }
    let name =
        if command.ends_with(std::env::consts::EXE_SUFFIX) { command.to_string() } else { format!("{command}{}", std::env::consts::EXE_SUFFIX) };
    path_var.is_some_and(|p| std::env::split_paths(p).any(|d| file_access::exists(d.join(&name))))
}

/// Whether `command` is a CLI that has `mcp --project`: an older one installed earlier on PATH
/// would refuse the flag, and the client's server would never start.
#[cfg(not(target_arch = "wasm32"))]
pub fn understands_project(command: &str) -> bool {
    use std::process::{Command, Stdio};
    let mut run = Command::new(command);
    if command.ends_with(".AppImage") {
        run.arg("cli");
    }
    let Ok(mut child) = run.args(["mcp", "--help"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return false };
    // Something else of that name mustn't hold the window up.
    let since = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if since.elapsed() < CLI_WAIT => std::thread::sleep(std::time::Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
    child.wait_with_output().is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("--project"))
}

/// How to start the CLI: by name when it's on `path_var` and `usable` says it fits, else by path,
/// beside the program `exe` or through the AppImage it came in. `None` when there is none to find.
pub fn cli_entry(
    exe: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
    appimage: Option<&std::ffi::OsStr>,
    usable: &dyn Fn(&str) -> bool,
) -> Option<McpEntry> {
    if runnable("figcraft-cli", path_var) && usable("figcraft-cli") {
        return Some(McpEntry::new("figcraft-cli".into(), &[], false));
    }
    if let Some(image) = appimage.and_then(std::ffi::OsStr::to_str) {
        return Some(McpEntry::new(image.to_string(), &["cli"], true));
    }
    let beside = exe?.parent()?.join(cli_name());
    let beside = beside.to_str().filter(|_| file_access::exists(&beside))?.to_string();
    usable(&beside).then(|| McpEntry::new(beside, &[], true))
}

// ------------------------------------------------------------------ editing JSON text

/// A member of an object in a JSON text.
struct Member {
    key: String,
    value: Range<usize>,
}

/// The first byte at or after `i` that isn't JSON white space.
fn skip_space(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n')) {
        i += 1;
    }
    i
}

/// Where the string opening at `i` ends (just past its closing quote).
fn string_end(b: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    loop {
        match b.get(i)? {
            b'"' => return Some(i + 1),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
}

/// Where the value starting at `i` ends.
fn value_end(b: &[u8], mut i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            loop {
                match b.get(i)? {
                    b'"' => {
                        i = string_end(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        _ => {
            while b.get(i).is_some_and(|c| !matches!(c, b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')) {
                i += 1;
            }
            Some(i)
        }
    }
}

/// The members of the object opening at `open` in valid JSON `text`, and where its `}` is.
fn members(text: &str, open: usize) -> Option<(Vec<Member>, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = skip_space(b, open + 1);
    while *b.get(i)? != b'}' {
        let end = string_end(b, i)?;
        let key: String = serde_json::from_str(text.get(i..end)?).ok()?;
        let start = skip_space(b, skip_space(b, end) + 1);
        let stop = value_end(b, start)?;
        out.push(Member { key, value: start..stop });
        i = skip_space(b, stop);
        if *b.get(i)? == b',' {
            i = skip_space(b, i + 1);
        }
    }
    Some((out, i))
}

/// The white space before the line starting at or before `at`.
fn line_pad(text: &str, at: usize) -> &str {
    let line = text.get(..at).and_then(|t| t.rfind('\n')).map_or(0, |n| n + 1);
    let rest = text.get(line..).unwrap_or_default();
    rest.get(..rest.len() - rest.trim_start_matches([' ', '\t']).len()).unwrap_or_default()
}

/// `text` with member `key` added to the object opening at `open`, `render` giving its value for
/// the indentation it lands at.
fn add_member(text: &str, open: usize, key: &str, render: impl Fn(&str) -> String) -> Option<String> {
    let (list, close) = members(text, open)?;
    let quoted = Value::from(key).to_string();
    let spliced = |at: Range<usize>, new: String| Some(format!("{}{new}{}", text.get(..at.start)?, text.get(at.end..)?));
    let Some(last) = list.last() else {
        let outer = line_pad(text, open);
        let pad = format!("{outer}  ");
        return spliced(open + 1..close, format!("\n{pad}{quoted}: {}\n{outer}", render(&pad)));
    };
    let first = list.first().map_or(close, |m| m.value.start);
    let lead = text.get(open + 1..first).unwrap_or_default();
    let new = if lead.contains('\n') {
        let pad = line_pad(text, first);
        format!(",\n{pad}{quoted}: {}", render(pad))
    } else {
        format!(", {quoted}: {}", render(line_pad(text, open)))
    };
    spliced(last.value.end..last.value.end, new)
}

/// Where the top-level object of valid JSON `text` opens.
fn object_start(text: &str) -> usize {
    skip_space(text.as_bytes(), 0)
}

// ------------------------------------------------------------------ the client's configuration

/// What setup did to a client's configuration file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Updated,
    Unchanged,
}

/// A client configuration file after merging FigCraft's entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub change: Change,
    pub warnings: Vec<String>,
}

/// Whether server entry `v` runs FigCraft's project server (from any path).
fn is_project_server(v: &Value) -> bool {
    let args: Vec<&str> = v.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    v.get("command").and_then(Value::as_str).is_some() && args.windows(2).any(|w| w == MCP_ARGS)
}

/// A new configuration file.
fn fresh_config(client: &Client, entry: &McpEntry) -> String {
    format!("{{\n  {}: {{\n    {}: {}\n  }}\n}}\n", Value::from(client.servers_key), Value::from(SERVER_NAME), entry.text("    "))
}

/// `existing` (a client's configuration text, if any) with FigCraft's entry in it, every other byte
/// as it was. An entry that already runs the project server and starts here stays, so a file
/// committed by someone whose CLI is elsewhere doesn't change back and forth. `Err` says why the
/// file can't be merged; it must then be left alone.
pub fn merge_config(existing: Option<&str>, client: &Client, entry: &McpEntry, runs: &dyn Fn(&str) -> bool) -> Result<Merged, String> {
    let name = client.config;
    let Some(text) = existing.filter(|t| !t.trim().is_empty()) else {
        return Ok(Merged { text: fresh_config(client, entry), change: Change::Added, warnings: vec![] });
    };
    let root: Value = serde_json::from_str(text).map_err(|e| format!("{name} isn't valid JSON ({e}): fix it, then connect again"))?;
    let Value::Object(top) = &root else { return Err(format!("{name} isn't a JSON object")) };
    let key = client.servers_key;
    let servers: &Map<String, Value> = match top.get(key) {
        None => &Map::new(),
        Some(Value::Object(s)) => s,
        Some(_) => return Err(format!("`{key}` in {name} isn't an object")),
    };
    let warnings: Vec<String> = servers
        .iter()
        .filter(|(n, v)| *n != SERVER_NAME && v.get("command").and_then(Value::as_str).is_some_and(|c| c.contains("figcraft-cli")))
        .map(|(n, _)| format!("{name} also has a server named `{n}` that runs figcraft-cli: an agent will see FigCraft twice until you remove it"))
        .collect();
    let keep = |t: String, change| Merged { text: t, change, warnings: warnings.clone() };
    let current = servers.get(SERVER_NAME);
    if current.is_some_and(|c| is_project_server(c) && c.get("command").and_then(Value::as_str).is_some_and(runs)) {
        return Ok(keep(text.to_string(), Change::Unchanged));
    }
    let unsupported = || format!("couldn't edit {name} without changing the rest of it: add the `{SERVER_NAME}` server by hand");
    let open = object_start(text);
    let (list, _) = members(text, open).ok_or_else(unsupported)?;
    let spliced = match list.iter().rev().find(|m| m.key == key) {
        None => add_member(text, open, key, |pad| {
            let inner = format!("{pad}  ");
            format!("{{\n{inner}{}: {}\n{pad}}}", Value::from(SERVER_NAME), entry.text(&inner))
        }),
        Some(servers_at) => {
            let (inside, _) = members(text, servers_at.value.start).ok_or_else(unsupported)?;
            match inside.iter().rev().find(|m| m.key == SERVER_NAME) {
                None => add_member(text, servers_at.value.start, SERVER_NAME, |pad| entry.text(pad)),
                Some(m) => {
                    let pad = line_pad(text, m.value.start);
                    Some(format!(
                        "{}{}{}",
                        text.get(..m.value.start).unwrap_or_default(),
                        entry.text(pad),
                        text.get(m.value.end..).unwrap_or_default()
                    ))
                }
            }
        }
    }
    .ok_or_else(unsupported)?;
    // What was written must be what was meant, whatever the file looked like.
    let mut wanted = root.clone();
    if let Some(w) = wanted.as_object_mut() {
        let list = w.entry(key).or_insert_with(|| json!({}));
        list[SERVER_NAME] = entry.json();
    }
    if serde_json::from_str::<Value>(&spliced).ok().as_ref() != Some(&wanted) {
        return Err(unsupported());
    }
    Ok(keep(spliced, if current.is_some() { Change::Updated } else { Change::Added }))
}

// ------------------------------------------------------------------ the project

/// What `figcraft.connect.json` says about the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub name: Option<String>,
    pub asset_dir: String,
    /// Code Connect links in the file.
    pub links: usize,
}

impl Settings {
    /// The settings in connect file text (`None`: nothing readable, the defaults).
    pub fn parse(text: Option<&str>) -> Self {
        let v: Value = text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
        let project = v.get("project");
        let field = |k: &str| project.and_then(|p| p.get(k)).and_then(Value::as_str).map(str::to_string);
        Self {
            name: field("name").filter(|n| !n.trim().is_empty()),
            asset_dir: field("assetDir").filter(|d| check_asset_dir(d).is_ok()).unwrap_or_else(|| DEFAULT_ASSET_DIR.to_string()),
            links: v.get("links").and_then(Value::as_array).map_or(0, Vec::len),
        }
    }
}

/// Why `dir` can't be an asset folder: it must be a relative path going down from the project.
pub fn check_asset_dir(dir: &str) -> Result<(), String> {
    let path = Path::new(dir);
    let plain = !dir.trim().is_empty()
        && dir.chars().count() <= MAX_ASSET_DIR
        && !dir.contains('\\')
        && !dir.chars().any(char::is_control)
        && path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if plain { Ok(()) } else { Err(format!("the asset folder must be a path inside the project, like {DEFAULT_ASSET_DIR}")) }
}

/// A project as a connection sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// The clients configured to reach FigCraft here.
    pub clients: Vec<&'static str>,
    pub settings: Settings,
    /// `figcraft.connect.json` exists.
    pub connect_file: bool,
    /// Why something here doesn't work.
    pub problem: Option<String>,
}

/// The text of `path`, `None` when there is no such file.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    if !file_access::exists(path) {
        return Ok(None);
    }
    let bytes = file_access::read_capped(path, MAX_CONFIG).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map(Some).map_err(|_| format!("{} isn't text", path.display()))
}

/// What `project` has set up. Reads only.
pub fn status(project: &Path) -> Status {
    if !file_access::is_dir(project) {
        let problem = format!("the folder {} is gone", project.display());
        return Status { clients: vec![], settings: Settings::parse(None), connect_file: false, problem: Some(problem) };
    }
    let connect = read_text(&project.join(CONNECT_FILE));
    let mut problem = connect.as_ref().err().cloned();
    let text = connect.ok().flatten();
    if let Some(t) = text.as_deref().filter(|t| serde_json::from_str::<Value>(t).is_err()) {
        problem = Some(format!("{CONNECT_FILE} isn't valid JSON ({} bytes)", t.len()));
    }
    let clients = CLIENTS
        .iter()
        .filter(|c| {
            let config = read_text(&project.join(c.config)).ok().flatten().and_then(|t| serde_json::from_str::<Value>(&t).ok());
            config.and_then(|v| v.get(c.servers_key).and_then(|s| s.get(SERVER_NAME)).map(is_project_server)).unwrap_or(false)
        })
        .map(|c| c.label)
        .collect();
    Status { clients, connect_file: text.is_some(), settings: Settings::parse(text.as_deref()), problem }
}

/// The project folder name.
fn folder_name(project: &Path) -> String {
    project.file_name().map_or_else(|| "project".to_string(), |n| n.to_string_lossy().into_owned())
}

/// The nearest folder at or above `start` with a `figcraft.connect.json`.
pub fn find_project(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| file_access::exists(d.join(CONNECT_FILE))).map(Path::to_path_buf)
}

/// The project's `project` object, ready for a connect file.
fn project_text(project: &Path, pad: &str) -> String {
    format!("{{\n{pad}  \"name\": {},\n{pad}  \"assetDir\": {}\n{pad}}}", Value::from(folder_name(project)), Value::from(DEFAULT_ASSET_DIR))
}

/// `figcraft.connect.json` for `project`: made when missing, its `project` key added when it has
/// none (mappings stay as they are). Returns whether it changed.
fn ensure_connect_file(project: &Path, warnings: &mut Vec<String>) -> Result<bool, String> {
    let path = project.join(CONNECT_FILE);
    let new = match read_text(&path)? {
        None => format!("{{\n  \"version\": 1,\n  \"project\": {},\n  \"links\": []\n}}\n", project_text(project, "  ")),
        Some(t) => {
            let parsed: Result<Value, _> = serde_json::from_str(&t);
            match parsed {
                Ok(Value::Object(o)) if o.contains_key("project") => return Ok(false),
                Ok(Value::Object(_)) => match add_member(&t, object_start(&t), "project", |pad| project_text(project, pad)) {
                    Some(new) => new,
                    None => {
                        warnings.push(format!("couldn't add the project's settings to {CONNECT_FILE}: left as it is"));
                        return Ok(false);
                    }
                },
                _ => {
                    warnings.push(format!("{CONNECT_FILE} isn't a JSON object: left as it is"));
                    return Ok(false);
                }
            }
        }
    };
    file_access::write(&path, new.as_bytes()).map_err(|e| e.to_string())?;
    Ok(true)
}

/// What [`setup`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub client: Change,
    pub connect_file: bool,
    pub warnings: Vec<String>,
}

/// Roots keeping work to `project`: links out of it are refused, like an agent's would be.
pub fn project_roots(project: &Path) -> Result<Arc<Roots>, String> {
    let dir = project.to_string_lossy().into_owned();
    Roots::new(&[], std::slice::from_ref(&dir)).map(Arc::new).map_err(|e| e.replace("--automation-write-root ", ""))
}

/// Set `project` up for `client`, confined to the project. Running it again changes nothing.
pub fn setup(project: &Path, client: &Client, entry: &McpEntry, usable: &dyn Fn(&str) -> bool) -> Result<Setup, String> {
    // Asked before the confinement: the CLI is outside the project.
    let path_var = std::env::var_os("PATH");
    let runs = |c: &str| runnable(c, path_var.as_deref()) && usable(c);
    let entry = &entry.for_client(client);
    let roots = project_roots(project)?;
    let config = project.join(client.config);
    let existing = file_access::confined(Some(&roots), || read_text(&config))?;
    let merged = merge_config(existing.as_deref(), client, entry, &runs)?;
    file_access::confined(Some(&roots), || {
        if merged.change != Change::Unchanged {
            if let Some(dir) = config.parent() {
                file_access::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            file_access::write(&config, merged.text.as_bytes()).map_err(|e| e.to_string())?;
        }
        let mut warnings = merged.warnings.clone();
        let connect_file = ensure_connect_file(project, &mut warnings)?;
        if entry.machine_specific && merged.change != Change::Unchanged {
            warnings.push(format!(
                "{} names a path on this computer, because figcraft-cli isn't on PATH: put it on PATH before committing this file",
                client.config
            ));
        }
        Ok(Setup { client: merged.change, connect_file, warnings })
    })
}

/// Point the project's assets at `dir` (relative to the project; a link out of it is refused).
pub fn set_asset_dir(project: &Path, dir: &str) -> Result<(), String> {
    check_asset_dir(dir)?;
    let roots = project_roots(project)?;
    file_access::confined(Some(&roots), || {
        roots.check(&project.join(dir), Access::Write)?;
        let path = project.join(CONNECT_FILE);
        let text = read_text(&path)?.ok_or_else(|| format!("{CONNECT_FILE} is missing: connect the project again"))?;
        let open = object_start(&text);
        let (top, _) = members(&text, open).ok_or_else(|| format!("{CONNECT_FILE} isn't a JSON object"))?;
        let project_at =
            top.iter().rev().find(|m| m.key == "project").ok_or_else(|| format!("{CONNECT_FILE} has no `project`: connect the project again"))?;
        if !text.as_bytes().get(project_at.value.start).is_some_and(|c| *c == b'{') {
            return Err(format!("`project` in {CONNECT_FILE} isn't an object"));
        }
        let (inside, _) = members(&text, project_at.value.start).ok_or_else(|| format!("{CONNECT_FILE} isn't valid"))?;
        let quoted = Value::from(dir).to_string();
        let new = match inside.iter().rev().find(|m| m.key == "assetDir") {
            Some(m) => Some(format!("{}{quoted}{}", text.get(..m.value.start).unwrap_or_default(), text.get(m.value.end..).unwrap_or_default())),
            None => add_member(&text, project_at.value.start, "assetDir", |_| quoted.clone()),
        };
        let new = new.ok_or_else(|| format!("couldn't edit {CONNECT_FILE}"))?;
        file_access::write(&path, new.as_bytes()).map_err(|e| e.to_string())
    })
}

/// What `info` tells an agent about `project`.
pub fn info(project: &Path, name: &str) -> Value {
    let settings = status(project).settings;
    json!({"name": settings.name.unwrap_or_else(|| name.to_string()), "assetDir": settings.asset_dir, "links": settings.links})
}

//! Developer Connect's listener: a second control socket beside `--control`, started from the
//! window on a free port. A connection must first send `auth`, as the suite's other control channels
//! ask, with this run's token and a connected project; it then speaks the control protocol confined to that project
//! ([`control_server::Scope`]). The port and token are in a discovery file only this user can read,
//! which goes when the app stops. See `docs/developer-integration/ARCHITECTURE.md`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use figcraft_engine::events::EventLog;
use figcraft_engine::project_connect::{self, DISCOVERY_VERSION, Instance};
use figcraft_ui_egui::ControlRequest;
use figcraft_ui_egui::connect::Hub;
use serde_json::{Value, json};

use crate::control_server::{self, CtxSlot, Queue, Scope};

/// Most agents connected at once.
const MAX_LINKS: usize = 8;
/// How long a new connection has to introduce itself.
const AUTH_WAIT: Duration = Duration::from_secs(5);
/// Longest `auth` line.
const MAX_AUTH: usize = 64 * 1024;
/// How long a discovery file's port gets to answer before the file counts as left by a crash.
const STALE_WAIT: Duration = Duration::from_millis(200);
/// How long a request waits for the UI, as on `--control`.
const TIMEOUT: Duration = Duration::from_secs(60);

struct Running {
    port: u16,
    stop: Arc<AtomicBool>,
    file: PathBuf,
}

/// What the listener needs from the app, and the listener once it runs.
pub struct Launcher {
    queue: Queue,
    ctx: CtxSlot,
    events: Arc<EventLog>,
    running: Option<Running>,
}

impl Launcher {
    pub fn new(queue: Arc<Mutex<Vec<ControlRequest>>>, ctx: CtxSlot, events: Arc<EventLog>) -> Self {
        Self { queue, ctx, events, running: None }
    }

    /// Listen on a free loopback port (the one already in use when it runs) and say so in the
    /// discovery file.
    pub fn start(&mut self, hub: Arc<Hub>) -> Result<u16, String> {
        if let Some(r) = &self.running {
            return Ok(r.port);
        }
        let dir = crate::services::config_dir()
            .ok_or("no folder for the app's settings: set HOME or FIGCRAFT_CONFIG_DIR")?
            .join(project_connect::INSTANCES_FOLDER);
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("can't open a local port: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = new_token()?;
        clear_stale(&dir);
        let file = publish(&dir, port, &token).map_err(|e| format!("can't write {}: {e}", dir.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (queue, ctx, events) = (self.queue.clone(), self.ctx.clone(), self.events.clone());
        let stopped = stop.clone();
        let spawned = std::thread::Builder::new().name("figcraft-connect".into()).spawn(move || {
            accept(&listener, &token, &stopped, &hub, &queue, &ctx, &events);
        });
        if let Err(e) = spawned {
            let _ = std::fs::remove_file(&file);
            return Err(e.to_string());
        }
        self.running = Some(Running { port, stop, file });
        Ok(port)
    }

    /// Stop listening and take the discovery file away.
    pub fn stop(&mut self) {
        let Some(r) = self.running.take() else { return };
        let _ = std::fs::remove_file(&r.file);
        r.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread, which then sees `stop`.
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], r.port)), STALE_WAIT);
    }
}

/// 256 random bits as hex: a new one every run, so a file left by a crash opens nothing.
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random numbers from the system: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Tokens compared without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Write this run's discovery file; only its user can read it.
fn publish(dir: &Path, port: u16, token: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let instance = Instance {
        version: DISCOVERY_VERSION,
        pid: std::process::id(),
        port,
        token: token.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let file = dir.join(format!("{}.json", instance.pid));
    crate::services::write_private(&file, serde_json::to_string(&instance).map_err(std::io::Error::other)?.as_bytes())?;
    Ok(file)
}

/// Remove the discovery files of apps that are gone (a crash leaves its file).
fn clear_stale(dir: &Path) {
    for file in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
        let alive = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Instance>(&b).ok())
            .is_some_and(|i| TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], i.port)), STALE_WAIT).is_ok());
        if !alive {
            let _ = std::fs::remove_file(file);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn accept(listener: &TcpListener, token: &str, stop: &AtomicBool, hub: &Arc<Hub>, queue: &Queue, ctx: &CtxSlot, events: &Arc<EventLog>) {
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    for stream in listener.incoming().flatten() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        workers.retain(|w| !w.is_finished());
        if workers.len() >= MAX_LINKS {
            let mut stream = stream;
            let _ = writeln!(
                stream,
                "{}",
                json!({"id": null, "ok": false, "error": format!("FigCraft has {MAX_LINKS} agents connected: close one and try again")})
            );
            continue;
        }
        let (token, hub, queue, ctx, events) = (token.to_string(), hub.clone(), queue.clone(), ctx.clone(), events.clone());
        match std::thread::Builder::new().name("figcraft-connect-link".into()).spawn(move || {
            let _ = serve(stream, &token, &hub, queue, ctx, events);
        }) {
            Ok(worker) => workers.push(worker),
            Err(e) => log::error!("developer connect thread: {e}"),
        }
    }
}

/// The first line, within [`AUTH_WAIT`] in all however slowly it comes. `None`: too slow, too long
/// or not there.
fn read_auth(reader: &mut BufReader<TcpStream>) -> Option<Value> {
    let since = Instant::now();
    let mut line = Vec::new();
    loop {
        let left = AUTH_WAIT.checked_sub(since.elapsed()).filter(|d| !d.is_zero())?;
        reader.get_ref().set_read_timeout(Some(left)).ok()?;
        let chunk = reader.fill_buf().ok()?;
        if chunk.is_empty() {
            return None;
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let used = end.map_or(chunk.len(), |n| n + 1);
        line.extend_from_slice(chunk.get(..used)?);
        reader.consume(used);
        if line.len() > MAX_AUTH {
            return None;
        }
        if end.is_some() {
            return serde_json::from_slice(&line).ok();
        }
    }
}

/// The handshake: `Ok` with the project and client it names, `Err` with what it is told.
fn check_auth(auth: &Value, token: &str) -> Result<(String, String), String> {
    let p = auth.get("params");
    let field = |k: &str| p.and_then(|p| p.get(k)).and_then(Value::as_str);
    if auth.get("method").and_then(Value::as_str) != Some("auth") || !field("token").is_some_and(|t| same(t, token)) {
        return Err("not authorized".into());
    }
    let project = field("project").ok_or("`project` required")?;
    let client: String = field("client").filter(|c| !c.trim().is_empty()).unwrap_or("AI agent").chars().take(64).collect();
    Ok((project.to_string(), client))
}

/// Ends the link when its connection ends, however it ends.
struct Unlink<'a>(&'a Hub, u64);

impl Drop for Unlink<'_> {
    fn drop(&mut self) {
        self.0.unlink(self.1);
    }
}

fn serve(stream: TcpStream, token: &str, hub: &Hub, queue: Queue, ctx: CtxSlot, events: Arc<EventLog>) -> std::io::Result<()> {
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(auth) = read_auth(&mut reader) else {
        return writeln!(out, "{}", json!({"id": null, "ok": false, "error": "not authorized"}));
    };
    let id = auth.get("id").cloned().unwrap_or(Value::Null);
    let linked =
        check_auth(&auth, token).and_then(|(project, client)| hub.link(&project, &client, Box::new(move || drop(stream.shutdown(Shutdown::Both)))));
    let ticket = match linked {
        Ok(ticket) => ticket,
        Err(error) => return writeln!(out, "{}", json!({"id": id, "ok": false, "error": error})),
    };
    let _unlink = Unlink(hub, ticket.link);
    let scope = Scope { hub, link: ticket.link, roots: ticket.roots, path: ticket.path, name: ticket.name };
    writeln!(out, "{}", json!({"id": id, "ok": true, "result": scope.info()}))?;
    // The agent's process holds this open as long as it lives: an idle agent is still connected,
    // and the system ends the socket when the process goes.
    reader.get_ref().set_read_timeout(None)?;
    control_server::serve_lines(reader, out, queue, ctx, events, TIMEOUT, Some(&scope))
}

#[cfg(test)]
#[path = "connect_server_tests.rs"]
mod tests;

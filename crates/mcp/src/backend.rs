//! Where MCP tool calls end up: a control-channel method call, answered either by an in-process
//! engine session ([`crate::Headless`]) or by a running FigCraft app ([`Attached`]).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};

/// Something that answers the FigCraft control-channel methods (`engine.execute`,
/// `engine.commands`, `document.inspect`, `node.get`, `ui.pointer`, `ui.key`, `ui.type`,
/// `ui.render`, `ui.screenshot`, `ui.inspect`). See `docs/control-protocol.md`.
pub trait Backend {
    /// Call one method. `Ok` carries the `result`, `Err` the error message.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;

    /// True when a real UI window is attached (`ui.screenshot` captures the window).
    fn has_ui(&self) -> bool;

    /// Short human description ("headless", "attached to 127.0.0.1:7979").
    fn describe(&self) -> String;

    /// The MCP client's name, as `initialize` gives it.
    fn set_client(&mut self, _client: &str) {}

    /// The connected project's asset folder, relative to the project.
    fn asset_dir(&mut self) -> Option<String> {
        None
    }

    /// `path` against the connected project, when there is one and it's relative.
    fn project_path(&self, path: &str) -> String {
        path.to_string()
    }

    /// How this backend reaches a document now, for the `connection_status` tool. Asks again, so it
    /// also reconnects.
    fn connection(&mut self) -> Value {
        json!({"mode": "headless", "connected": true})
    }

    /// PNG bytes of the document: `ids` (in page coordinates, their union) or, when empty, the
    /// current page's content. The default goes through `ui.render`, the PNG coming back as data.
    fn render_png(&mut self, ids: &[u64], scale: f64) -> Result<Vec<u8>, String> {
        let mut params = json!({"scale": scale});
        if !ids.is_empty() {
            params["ids"] = json!(ids);
        } else {
            // `ui.render` without ids exports the selection; pass the page's children instead.
            let kids = page_children(self)?;
            if kids.is_empty() {
                return Err("the current page is empty: nothing to render".into());
            }
            params["ids"] = json!(kids);
        }
        decode_data(&self.call("ui.render", params)?)
    }

    /// PNG bytes of what the user sees: the app window when attached, the current viewport when
    /// headless.
    fn screenshot_png(&mut self) -> Result<Vec<u8>, String> {
        capture(self, json!({}))
    }

    /// PNG bytes of the attached app's window `window` (an id from `window.list`).
    fn window_png(&mut self, window: u64) -> Result<Vec<u8>, String> {
        capture(self, json!({"window": window}))
    }
}

/// A window capture, the PNG coming back as data (no file: an app confined to automation roots
/// writes none outside them).
fn capture<B: Backend + ?Sized>(b: &mut B, params: Value) -> Result<Vec<u8>, String> {
    decode_data(&b.call("ui.screenshot", params)?)
}

/// The ids of the current page's top-level layers.
pub fn page_children<B: Backend + ?Sized>(b: &mut B) -> Result<Vec<u64>, String> {
    let doc = b.call("document.inspect", json!({"depth": 1}))?;
    let current = doc.get("currentPage").cloned().unwrap_or(Value::Null);
    let pages = doc.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
    let page = pages.iter().find(|p| p.get("id") == Some(&current)).or_else(|| pages.first());
    Ok(page
        .and_then(|p| p.get("children"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.get("id").and_then(Value::as_u64)).collect())
        .unwrap_or_default())
}

/// Decode a `data` field (base64, optionally a data: URL) from an export result.
pub fn decode_data(v: &Value) -> Result<Vec<u8>, String> {
    let d = v.get("data").and_then(Value::as_str).ok_or("the export returned no data")?;
    let d = d.split_once(',').map(|(_, b)| b).unwrap_or(d);
    base64::engine::general_purpose::STANDARD.decode(d.trim()).map_err(|e| format!("bad base64 from the app: {e}"))
}

/// The longest reply line we read from the app (512 MiB). The app's window captures alone reach
/// 256 MiB as base64 (192 MiB PNGs); anything longer is refused without reading the rest.
const MAX_REPLY: usize = 512 * 1024 * 1024;

/// A project an [`Attached`] backend reaches through whichever app has it connected.
struct Project {
    path: PathBuf,
    /// Where apps leave their discovery files.
    dir: Option<PathBuf>,
    client: String,
    /// The app's last answer about the project.
    info: Value,
}

/// A running FigCraft app, reached through its loopback control port (`figcraft --control PORT`),
/// or found for a connected project (Developer Connect).
pub struct Attached {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    /// [`MAX_REPLY`]; smaller in tests.
    max_reply: usize,
    /// How long the app is asked to wait for each request; `None`: its own 60 s.
    timeout: Option<Duration>,
    /// Set: `addr` isn't fixed, each connection finds the app again.
    project: Option<Project>,
}

impl Attached {
    /// Connect to `addr` (e.g. `127.0.0.1:7979`), failing fast when nothing is listening.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1, max_reply: MAX_REPLY, timeout: None, project: None };
        r.reconnect()?;
        Ok(r)
    }

    /// A backend for the project at `path`, with apps' discovery files in `dir`, connecting when it
    /// is first needed: the app may open later, and close and come back, and the server keeps
    /// serving meanwhile.
    pub fn project(path: PathBuf, dir: Option<PathBuf>) -> Self {
        let project = Project { path, dir, client: String::new(), info: Value::Null };
        Self { addr: String::new(), conn: None, next_id: 1, max_reply: MAX_REPLY, timeout: None, project: Some(project) }
    }

    /// Ask the app to wait up to `timeout` for each request instead of its 60 s (it caps this; an
    /// app from before this ignores it and answers within its 60 s), and read its answers as long.
    pub fn with_timeout(mut self, timeout: Duration) -> std::io::Result<Self> {
        self.timeout = Some(timeout);
        if self.project.is_none() {
            self.reconnect()?;
        }
        Ok(self)
    }

    /// Connect to `127.0.0.1:port`.
    pub fn port(port: u16) -> std::io::Result<Self> {
        Self::connect(&format!("127.0.0.1:{port}"))
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// How long a reply may take: the app answers within its timeout; leave headroom.
    fn read_timeout(&self) -> Duration {
        self.timeout.unwrap_or(Duration::from_secs(60)).saturating_add(Duration::from_secs(30))
    }

    /// Connect to the app that has the project connected, reading its discovery file afresh.
    fn relink(&mut self) -> std::io::Result<()> {
        let wait = self.read_timeout();
        let Some(p) = self.project.as_mut() else { return Err(std::io::Error::other("no project")) };
        let client = if p.client.is_empty() { "AI agent" } else { p.client.as_str() };
        let link = crate::connect::link(p.dir.as_deref(), &p.path, client).map_err(std::io::Error::other)?;
        link.stream.set_read_timeout(Some(wait))?;
        p.info = link.info;
        self.addr = format!("127.0.0.1:{}", link.port);
        self.conn = Some((link.reader, link.stream));
        Ok(())
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        if self.project.is_some() {
            return self.relink();
        }
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(self.read_timeout())).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// The app already closed this connection (it drops ones idle past its socket timeout), or
    /// left bytes on it nobody asked for. Checked before a request is written, when replacing the
    /// connection can't repeat anything.
    fn stale(&mut self) -> bool {
        let Some((reader, writer)) = self.conn.as_mut() else { return false };
        if !reader.buffer().is_empty() || writer.set_nonblocking(true).is_err() {
            return true;
        }
        // Nothing to read on a healthy idle connection: the peek would block.
        let idle = matches!(writer.peek(&mut [0]), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock);
        writer.set_nonblocking(false).is_err() || !idle
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        let max_reply = self.max_reply;
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected to the app"));
        };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = Vec::new();
        if reader.take((max_reply + 1) as u64).read_until(b'\n', &mut reply)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control channel closed"));
        }
        if reply.last() == Some(&b'\n') {
            reply.pop();
        }
        if reply.len() > max_reply {
            let mib = max_reply / (1024 * 1024);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("reply larger than {mib} MiB")));
        }
        String::from_utf8(reply).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl Backend for Attached {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = json!({"id": id, "method": method, "params": params});
        if let (Some(t), Some(fields)) = (self.timeout, line.as_object_mut()) {
            fields.insert("timeoutMs".into(), json!(u64::try_from(t.as_millis()).unwrap_or(u64::MAX)));
        }
        let line = line.to_string();
        if self.conn.is_none() || self.stale() {
            self.reconnect().map_err(|e| match &self.project {
                Some(_) => format!("{e} (the request was not sent)"),
                None => {
                    format!("FigCraft app at {} is not reachable: {e} (request was not sent; start it with `figcraft --control PORT`)", self.addr)
                }
            })?;
        }
        // A lost reply does not mean the app failed to apply the request.
        let reply = self.roundtrip(&line).map_err(|e| {
            self.conn = None;
            format!(
                "FigCraft control call `{method}` at {} failed: {e}; the operation may have completed. Inspect the document before retrying.",
                self.addr
            )
        })?;
        let v: Value = serde_json::from_str(reply.trim()).map_err(|e| format!("bad reply from the app: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string())
        }
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        match &self.project {
            Some(p) => format!("Developer Connect for the project {} (connection_status says whether the app is there)", p.path.display()),
            None => format!("attached to the FigCraft app at {}", self.addr),
        }
    }

    fn asset_dir(&mut self) -> Option<String> {
        if self.conn.is_none() {
            let _ = self.reconnect();
        }
        self.project.as_ref()?.info["project"]["assetDir"].as_str().map(str::to_string)
    }

    fn project_path(&self, path: &str) -> String {
        match &self.project {
            Some(p) => p.path.join(path).to_string_lossy().into_owned(),
            None => path.to_string(),
        }
    }

    fn set_client(&mut self, client: &str) {
        if let Some(p) = &mut self.project {
            client.clone_into(&mut p.client);
            // So the app shows the agent from the start. The app may not be there yet: calls say so.
            let _ = self.reconnect();
        }
    }

    fn connection(&mut self) -> Value {
        let unusable = self.conn.is_none() || self.stale();
        let problem = if unusable { self.reconnect().err().map(|e| e.to_string()) } else { None };
        let Some(p) = &self.project else {
            return json!({"mode": "attached", "address": self.addr, "connected": problem.is_none(), "problem": problem});
        };
        let (path, info) = (p.path.display().to_string(), p.info.clone());
        if problem.is_some() {
            return json!({"mode": "project", "path": path, "connected": false, "problem": problem});
        }
        let doc = self.call("document.inspect", json!({"depth": 0})).unwrap_or(Value::Null);
        let selected = doc.get("selection").and_then(Value::as_array).map_or(0, Vec::len);
        json!({"mode": "project", "path": path, "connected": true, "project": info["project"], "files": info["files"], "app": info["app"], "document": {"name": doc.get("name"), "selected": selected}})
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;

//! Reaching a running app for a project: the discovery files it leaves
//! (`figcraft_engine::project_connect`) and the `auth` that opens a project connection.
//! See `docs/developer-integration/ARCHITECTURE.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use figcraft_engine::project_connect::{DISCOVERY_VERSION, Instance};
use serde_json::{Value, json};

/// Largest discovery file read.
const MAX_FILE: u64 = 4096;
/// Most discovery files tried.
const MAX_FILES: usize = 64;
/// Longest answer to `auth`.
const MAX_REPLY: u64 = 1024 * 1024;
/// How long a port gets to accept.
const CONNECT_WAIT: Duration = Duration::from_millis(800);
/// How long the app gets to answer `auth`.
const AUTH_WAIT: Duration = Duration::from_secs(5);

/// What to say when no app is there.
pub const NO_APP: &str = "FigCraft isn't running, or Developer Connect is off: open FigCraft, then Developer Connect (the ? button)";

/// A project connection, ready for control requests.
pub struct Link {
    pub stream: TcpStream,
    pub reader: BufReader<TcpStream>,
    pub port: u16,
    /// The app's answer: the project's name and settings, and the folders it may touch.
    pub info: Value,
}

/// The discovery files in `dir`, newest first.
fn instances(dir: &Path) -> Vec<Instance> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(_, p)| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(MAX_FILES);
    files
        .into_iter()
        .filter_map(|(_, p)| {
            let mut bytes = Vec::new();
            std::fs::File::open(p).ok()?.take(MAX_FILE).read_to_end(&mut bytes).ok()?;
            serde_json::from_slice::<Instance>(&bytes).ok()
        })
        .filter(|i| i.version == DISCOVERY_VERSION)
        .collect()
}

/// `auth` to one app. `Ok(Err(why))`: it answered, and said no.
fn auth(i: &Instance, project: &Path, client: &str) -> std::io::Result<Result<Link, String>> {
    let stream = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], i.port)), CONNECT_WAIT)?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(AUTH_WAIT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let line = json!({"id": "auth", "method": "auth", "params": {"token": i.token, "project": project.to_string_lossy(), "client": client}});
    (&stream).write_all(format!("{line}\n").as_bytes())?;
    let mut reply = Vec::new();
    (&mut reader).take(MAX_REPLY).read_until(b'\n', &mut reply)?;
    let v: Value = serde_json::from_slice(&reply).map_err(std::io::Error::other)?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Ok(Err(v.get("error").and_then(Value::as_str).unwrap_or("refused").to_string()));
    }
    let info = v.get("result").cloned().unwrap_or(Value::Null);
    Ok(Ok(Link { stream, reader, port: i.port, info }))
}

/// Connect to the first running app that has `project` connected, with its discovery files in `dir`
/// (the app's settings folder decides that: `None` when there is none). `Err` says what the person
/// has to do.
pub fn link(dir: Option<&Path>, project: &Path, client: &str) -> Result<Link, String> {
    let found = dir.map(instances).unwrap_or_default();
    let mut refused = None;
    for i in found {
        match auth(&i, project, client) {
            Ok(Ok(link)) => return Ok(link),
            Ok(Err(why)) => refused = Some(why),
            // A file a crash left, or an app that's going: the next may answer.
            Err(_) => {}
        }
    }
    Err(refused.unwrap_or_else(|| NO_APP.to_string()))
}

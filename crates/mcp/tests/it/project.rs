//! Developer Connect from the MCP side: `Attached::project` against a fake app that leaves a
//! discovery file and asks for `auth`, as the desktop app does.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use figcraft_engine::project_connect::Instance;
use figcraft_mcp::{Attached, Backend, Headless, Server};
use serde_json::{Value, json};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-mcp-project-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

struct FakeApp {
    file: PathBuf,
    stop: Arc<AtomicBool>,
    port: u16,
    /// Its open sockets, which end with a real app's process.
    open: Arc<Mutex<Vec<TcpStream>>>,
}

impl FakeApp {
    /// An app with `project` connected, leaving its file in `dir` as process `pid`.
    fn start(dir: &Path, pid: u32, token: &str, project: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let file = dir.join(format!("{pid}.json"));
        let i = Instance { version: 1, pid, port, token: token.into(), app_version: "test".into() };
        std::fs::write(&file, serde_json::to_vec(&i).unwrap()).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let open = Arc::new(Mutex::new(Vec::new()));
        let (flag, token, project, sockets) = (stop.clone(), token.to_string(), project.to_path_buf(), open.clone());
        std::thread::spawn(move || {
            let mut h = Headless::new();
            for stream in listener.incoming().flatten() {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                sockets.lock().unwrap().push(stream.try_clone().unwrap());
                let mut out = stream.try_clone().unwrap();
                let mut lines = BufReader::new(stream).lines();
                let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
                let p = &hello["params"];
                if p["token"] != token.as_str() {
                    writeln!(out, "{}", json!({"id": 0, "ok": false, "error": "not authorized"})).unwrap();
                    continue;
                }
                if p["project"] != project.to_str().unwrap() {
                    writeln!(out, "{}", json!({"id": 0, "ok": false, "error": "the project isn't connected: open Developer Connect"})).unwrap();
                    continue;
                }
                let info = json!({"project": {"name": "shop", "assetDir": "public/figcraft", "links": 0}, "files": {"read": [], "write": []}, "app": {"version": "test", "client": p["client"]}});
                writeln!(out, "{}", json!({"id": 0, "ok": true, "result": info})).unwrap();
                for line in lines {
                    let Ok(line) = line else { break };
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let v: Value = serde_json::from_str(&line).unwrap();
                    let reply = match h.call(v["method"].as_str().unwrap(), v["params"].clone()) {
                        Ok(r) => json!({"id": v["id"], "ok": true, "result": r}),
                        Err(e) => json!({"id": v["id"], "ok": false, "error": e}),
                    };
                    if writeln!(out, "{reply}").is_err() {
                        break;
                    }
                }
            }
        });
        Self { file, stop, port, open }
    }

    /// The app quits: its file goes, its port stops answering.
    fn quit(self) {
        std::fs::remove_file(&self.file).unwrap();
        self.stop.store(true, Ordering::SeqCst);
        for s in self.open.lock().unwrap().iter() {
            let _ = s.shutdown(Shutdown::Both);
        }
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn server(project: &Path, dir: &Path) -> Server {
    let mut s = Server::new(Box::new(Attached::project(project.to_path_buf(), Some(dir.to_path_buf()))));
    let init =
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"clientInfo": {"name": "probe", "title": "Claude Code"}}}).to_string();
    let r: Value = serde_json::from_str(&s.handle_line(&init).unwrap()).unwrap();
    assert!(r["result"]["instructions"].as_str().unwrap().contains("Developer Connect"), "{r}");
    s
}

fn call(s: &mut Server, tool: &str, args: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}}).to_string();
    let r: Value = serde_json::from_str(&s.handle_line(&line).unwrap()).unwrap();
    r["result"].clone()
}

fn text(r: &Value) -> &str {
    r["content"][0]["text"].as_str().unwrap()
}

fn status(s: &mut Server) -> Value {
    serde_json::from_str(text(&call(s, "connection_status", json!({})))).unwrap()
}

#[test]
fn the_server_outlives_the_app_and_finds_it_again() {
    let (dir, project) = (folder("life-dir"), folder("life-project"));
    let mut s = server(&project, &dir);
    // No app: the server answers, and says what to do.
    let down = call(&mut s, "inspect_ui", json!({}));
    assert_eq!(down["isError"], true);
    assert!(text(&down).contains("Developer Connect") && text(&down).contains("not sent"), "{down}");
    assert_eq!(status(&mut s)["connected"], false);

    let app = FakeApp::start(&dir, 1, "first", &project);
    let up = status(&mut s);
    assert_eq!((up["connected"].as_bool(), up["project"]["name"].as_str()), (Some(true), Some("shop")), "{up}");
    assert_eq!(up["app"]["client"], "Claude Code");
    assert_eq!(call(&mut s, "create_node", json!({"type": "frame", "width": 10, "height": 10}))["isError"], false);

    app.quit();
    assert_eq!(status(&mut s)["connected"], false);

    // A new run: another port and token, found from the new file.
    let _app = FakeApp::start(&dir, 2, "second", &project);
    assert_eq!(call(&mut s, "inspect_ui", json!({}))["isError"], false);
}

#[test]
fn a_stale_file_is_skipped_for_the_app_that_answers() {
    let (dir, project) = (folder("stale-dir"), folder("stale-project"));
    let dead = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let i = Instance { version: 1, pid: 9, port: dead, token: "old".into(), app_version: "0".into() };
    std::fs::write(dir.join("9.json"), serde_json::to_vec(&i).unwrap()).unwrap();
    std::fs::write(dir.join("junk.json"), "{").unwrap();
    let _app = FakeApp::start(&dir, 1, "live", &project);
    let mut s = server(&project, &dir);
    assert_eq!(status(&mut s)["connected"], true);
}

#[test]
fn the_app_with_the_project_is_the_one_used() {
    let (dir, project, other) = (folder("two-dir"), folder("two-project"), folder("two-other"));
    let _a = FakeApp::start(&dir, 1, "a", &other);
    let _b = FakeApp::start(&dir, 2, "b", &project);
    let mut s = server(&project, &dir);
    assert_eq!(status(&mut s)["connected"], true);
}

#[test]
fn an_app_without_the_project_says_how_to_connect_it() {
    let (dir, project, other) = (folder("no-dir"), folder("no-project"), folder("no-other"));
    let _a = FakeApp::start(&dir, 1, "a", &other);
    let mut s = server(&project, &dir);
    let r = call(&mut s, "inspect_ui", json!({}));
    assert!(text(&r).contains("isn't connected"), "{r}");
    assert!(status(&mut s)["problem"].as_str().unwrap().contains("Developer Connect"));
}

#[test]
fn nothing_in_a_missing_folder_is_a_failure_at_startup() {
    let mut s = server(&folder("gone-project"), Path::new("/nonexistent/connect"));
    assert_eq!(status(&mut s)["connected"], false);
}

#[test]
fn a_headless_server_reports_itself() {
    let mut s = Server::new(Box::new(Headless::new()));
    assert_eq!(status(&mut s)["mode"], "headless");
}

#[test]
fn assets_go_to_the_projects_asset_folder() {
    let (dir, project) = (folder("assets-dir"), folder("assets-project"));
    let _app = FakeApp::start(&dir, 1, "t", &project);
    let mut s = server(&project, &dir);
    let star = call(&mut s, "create_node", json!({"type": "star", "width": 24, "height": 24, "props": {"fill": "#112233"}}));
    let id = serde_json::from_str::<Value>(text(&star)).unwrap()["id"].clone();
    let listed = call(&mut s, "export_assets", json!({"ids": [id], "dryRun": true}));
    assert_eq!(listed["isError"], false, "{listed}");
    assert!(!project.join("public").exists(), "a dry run writes nothing");
    let done = call(&mut s, "export_assets", json!({"ids": [id]}));
    let v: Value = serde_json::from_str(text(&done)).unwrap();
    let path = v["assets"][0]["path"].as_str().unwrap();
    assert!(path.starts_with(project.join("public/figcraft").to_str().unwrap()), "{v}");
    assert!(Path::new(path).is_file());
    let custom = call(&mut s, "export_assets", json!({"ids": [id], "dir": "static/art"}));
    assert_eq!(custom["isError"], false, "{custom}");
    assert!(project.join("static/art").is_dir());
}

#[test]
fn a_headless_server_needs_a_folder_to_write_to() {
    let mut s = Server::new(Box::new(Headless::new()));
    assert_eq!(call(&mut s, "export_assets", json!({"dryRun": true}))["isError"], false);
    let r = call(&mut s, "export_assets", json!({}));
    assert!(text(&r).contains("pass `dir`"), "{r}");
}

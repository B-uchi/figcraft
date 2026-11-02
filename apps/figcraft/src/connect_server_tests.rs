#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use figcraft_engine::ConnectedProject;
use figcraft_ui_egui::ControlReply;

const TOKEN: &str = "abc123";

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-connect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

struct Rig {
    hub: Arc<Hub>,
    queue: Queue,
    client: TcpStream,
    reader: BufReader<TcpStream>,
    worker: std::thread::JoinHandle<()>,
}

/// A project connection's server end on a loopback pair, `project` connected.
fn rig(project: &Path) -> Rig {
    let hub = Arc::new(Hub::default());
    hub.set_projects(&[ConnectedProject { path: project.to_string_lossy().into_owned(), name: "shop".into() }]);
    let queue: Queue = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let server = listener.accept().unwrap().0;
    let (h, q) = (hub.clone(), queue.clone());
    let worker = std::thread::spawn(move || {
        let _ = serve(server, TOKEN, &h, q, Arc::new(Mutex::new(None)), Arc::new(EventLog::default()));
    });
    let reader = BufReader::new(client.try_clone().unwrap());
    Rig { hub, queue, client, reader, worker }
}

impl Rig {
    fn send(&mut self, line: &str) -> Value {
        writeln!(self.client, "{line}").unwrap();
        self.reply()
    }

    fn reply(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or(Value::Null)
    }

    fn hello(&mut self, project: &Path) -> Value {
        self.send(&json!({"id": 1, "method": "connect.hello", "params": {"token": TOKEN, "project": project, "client": "Claude Code"}}).to_string())
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.send(&json!({"id": 2, "method": method, "params": params}).to_string())
    }

    /// True when the server closed the connection.
    fn closed(&mut self) -> bool {
        let mut rest = String::new();
        matches!(self.reader.read_line(&mut rest), Ok(0))
    }
}

/// The UI's side, answering one request with `ok`.
fn answer(queue: Queue) -> std::thread::JoinHandle<ControlRequest> {
    std::thread::spawn(move || {
        for _ in 0..1000 {
            if let Some(r) = queue.lock().unwrap().pop() {
                r.reply.send(ControlReply { id: r.id.clone(), result: Ok(json!("done")) }).unwrap();
                return r;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("nothing queued");
    })
}

#[test]
fn a_wrong_token_opens_nothing() {
    let dir = folder("token");
    let mut r = rig(&dir);
    let reply = r.send(&json!({"id": 1, "method": "connect.hello", "params": {"token": "nope!!", "project": dir}}).to_string());
    assert_eq!((reply["ok"].as_bool(), reply["error"].as_str()), (Some(false), Some("not authorized")));
    assert!(r.closed());
    assert!(r.hub.clients(&dir).is_empty());
}

#[test]
fn a_web_page_stops_at_its_first_line() {
    let dir = folder("http");
    let mut r = rig(&dir);
    let reply = r.send("POST / HTTP/1.1");
    assert_eq!(reply["ok"], false);
    assert!(r.closed());
}

#[test]
fn any_other_first_method_is_refused_without_running() {
    let dir = folder("first");
    let mut r = rig(&dir);
    assert_eq!(r.call("app.quit", json!({}))["error"], "not authorized");
    assert!(r.queue.lock().unwrap().is_empty());
}

#[test]
fn a_project_that_isnt_connected_is_told_so() {
    let (dir, other) = (folder("granted"), folder("not-granted"));
    let mut r = rig(&dir);
    let reply = r.hello(&other);
    assert!(reply["error"].as_str().unwrap().contains("isn't connected"), "{reply}");
    assert!(r.closed());
}

#[test]
fn a_connected_project_gets_its_folder_and_nothing_else() {
    let dir = folder("info");
    let mut r = rig(&dir);
    let reply = r.hello(&dir);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"]["project"]["name"], "shop");
    assert_eq!(reply["result"]["files"]["write"], json!([dir]));
    assert_eq!(r.hub.clients(&dir), ["Claude Code"]);
    assert_eq!(r.call("connect.info", json!({}))["result"]["project"]["assetDir"], "public/figcraft");
}

#[test]
fn requests_carry_the_projects_confinement_and_run_as_an_agent() {
    let dir = folder("scope");
    let mut r = rig(&dir);
    r.hello(&dir);
    let ui = answer(r.queue.clone());
    assert_eq!(r.call("engine.execute", json!({"command": "doc.list"}))["result"], "done");
    let request = ui.join().unwrap();
    let roots = request.roots.expect("confined");
    assert_eq!(roots.to_json()["write"], json!([dir]));
    assert_eq!(request.params["agent"], "", "never as the user");
}

#[test]
fn methods_that_operate_the_app_are_refused() {
    let dir = folder("methods");
    let mut r = rig(&dir);
    r.hello(&dir);
    for method in ["app.quit", "ui.input", "ui.menu.invoke", "ui.set", "window.new", "window.focus", "no.such"] {
        let reply = r.call(method, json!({}));
        assert!(reply["error"].as_str().unwrap().contains("isn't available"), "{method}: {reply}");
    }
    assert!(r.queue.lock().unwrap().is_empty());
}

#[test]
fn a_screenshot_written_later_is_refused() {
    let dir = folder("shot");
    let mut r = rig(&dir);
    r.hello(&dir);
    assert!(r.call("ui.screenshot", json!({"path": "/tmp/x.png"}))["error"].as_str().unwrap().contains("no `path`"));
    assert!(r.queue.lock().unwrap().is_empty());
}

#[test]
fn disconnecting_the_project_ends_the_connection_at_once() {
    let dir = folder("revoke");
    let mut r = rig(&dir);
    r.hello(&dir);
    r.hub.set_projects(&[]);
    assert!(r.closed());
    r.worker.join().unwrap();
    assert!(r.hub.clients(&dir).is_empty());
}

#[test]
fn an_agent_that_goes_away_leaves_no_link_and_an_idle_one_stays() {
    let dir = folder("leave");
    let mut r = rig(&dir);
    r.hello(&dir);
    assert_eq!(r.call("connect.info", json!({}))["ok"], true);
    let Rig { hub, client, reader, worker, .. } = r;
    client.shutdown(Shutdown::Both).unwrap();
    drop((client, reader));
    worker.join().unwrap();
    assert!(hub.clients(&dir).is_empty());
}

#[test]
fn a_long_first_line_is_refused() {
    let dir = folder("long");
    let mut r = rig(&dir);
    let reply = r.send(&"x".repeat(MAX_HELLO + 2));
    assert_eq!(reply["error"], "not authorized");
}

#[test]
fn tokens_are_new_and_long_and_compared_whole() {
    let (a, b) = (new_token().unwrap(), new_token().unwrap());
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert!(same(&a, &a.clone()));
    assert!(!same(&a, &b));
    assert!(!same("ab", "abc"));
}

#[cfg(unix)]
#[test]
fn the_discovery_file_is_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = folder("private").join("Connect");
    let file = publish(&dir, 1234, "secret").unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    let i: Instance = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!((i.port, i.pid, i.token.as_str()), (1234, std::process::id(), "secret"));
}

#[test]
fn files_left_by_a_crash_go_and_a_live_app_stays() {
    let dir = folder("stale");
    let live = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = live.local_addr().unwrap().port();
    let keep = publish(&dir, port, "t").unwrap();
    // A port nothing listens on.
    let dead = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let gone = dir.join("1.json");
    std::fs::write(&gone, serde_json::to_vec(&Instance { version: 1, pid: 1, port: dead, token: "t".into(), app_version: "0".into() }).unwrap())
        .unwrap();
    let junk = dir.join("2.json");
    std::fs::write(&junk, "not json").unwrap();
    clear_stale(&dir);
    assert!(keep.exists());
    assert!(!gone.exists() && !junk.exists());
}

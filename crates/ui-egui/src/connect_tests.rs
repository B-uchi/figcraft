#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-hub-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn project(path: &Path) -> ConnectedProject {
    ConnectedProject { path: path.to_string_lossy().into_owned(), name: "shop".into() }
}

#[test]
fn only_a_connected_project_links_and_its_roots_are_its_own() {
    let (dir, other) = (folder("granted"), folder("other"));
    let hub = Hub::default();
    hub.set_projects(&[project(&dir)]);
    assert!(hub.link(other.to_str().unwrap(), "Claude Code", Box::new(|| {})).err().unwrap().contains("isn't connected"));
    let t = hub.link(dir.to_str().unwrap(), "Claude Code", Box::new(|| {})).unwrap();
    assert_eq!(t.path, dir);
    assert!(t.roots.check(&dir.join("a.png"), figcraft_engine::file_access::Access::Write).is_ok());
    assert!(t.roots.check(&other.join("a.png"), figcraft_engine::file_access::Access::Write).is_err());
    assert_eq!(hub.clients(&dir), ["Claude Code"]);
    hub.unlink(t.link);
    assert!(hub.clients(&dir).is_empty());
}

#[test]
fn disconnecting_a_project_ends_its_links_only() {
    static CLOSED: AtomicUsize = AtomicUsize::new(0);
    let (a, b) = (folder("a"), folder("b"));
    let hub = Hub::default();
    hub.set_projects(&[project(&a), project(&b)]);
    let close = || -> Box<dyn Fn() + Send> {
        Box::new(|| {
            CLOSED.fetch_add(1, Ordering::SeqCst);
        })
    };
    let ta = hub.link(a.to_str().unwrap(), "x", close()).unwrap();
    let tb = hub.link(b.to_str().unwrap(), "x", close()).unwrap();
    hub.set_projects(&[project(&b)]);
    assert!(!hub.linked(ta.link), "its access is gone at once");
    assert!(hub.linked(tb.link));
    assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
}

#[test]
fn a_project_that_links_through_a_symlink_is_the_same_project() {
    let dir = folder("real");
    let hub = Hub::default();
    hub.set_projects(&[project(&dir)]);
    let through = format!("{}/../{}", dir.display(), dir.file_name().unwrap().to_str().unwrap());
    assert!(hub.link(&through, "x", Box::new(|| {})).is_ok());
    assert!(hub.link("/definitely/not/here", "x", Box::new(|| {})).is_err());
}

#[test]
fn inspect_names_no_paths() {
    let dir = folder("inspect");
    let hub = Hub::default();
    hub.set_projects(&[project(&dir)]);
    assert!(!hub.inspect().to_string().contains(dir.to_str().unwrap()));
    assert_eq!(hub.inspect()["projects"], json!(["shop"]));
}

/// Services with the desktop app's project setup and a listener that always opens.
#[derive(Default)]
struct Desk {
    saved: std::rc::Rc<std::cell::Cell<usize>>,
}

impl crate::Services for Desk {
    fn connect_available(&self) -> bool {
        true
    }
    fn connect_start(&mut self, _hub: Arc<Hub>) -> Result<u16, String> {
        Ok(4242)
    }
    fn connect_cli(&self) -> Option<project_connect::McpEntry> {
        Some(project_connect::McpEntry {
            command: "figcraft-cli".into(),
            args: vec!["mcp".into(), "--project".into()],
            machine_specific: false,
            typed: false,
        })
    }
    fn connect_setup(
        &self,
        project: &Path,
        client: &project_connect::Client,
        entry: &project_connect::McpEntry,
    ) -> Result<project_connect::Setup, String> {
        project_connect::setup(project, client, entry, &|_| true)
    }
    fn save_prefs(&mut self, _prefs: &Prefs) {
        self.saved.set(self.saved.get() + 1);
    }
}

fn desk_app() -> (FigApp, std::rc::Rc<std::cell::Cell<usize>>) {
    let desk = Desk::default();
    let saved = desk.saved.clone();
    let mut session = figcraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    (FigApp::new(session, Box::new(desk)), saved)
}

#[test]
fn connecting_a_folder_sets_it_up_grants_it_and_remembers_it() {
    let dir = folder("connect");
    let (mut app, saved) = desk_app();
    app.connect_project(&dir);
    assert!(matches!(&app.ui.developer_connect_notice, Some(Ok(said)) if said.contains(".mcp.json")), "{:?}", app.ui.developer_connect_notice);
    assert!(dir.join(".mcp.json").exists() && dir.join(project_connect::CONNECT_FILE).exists());
    assert_eq!(app.connect.port(), Some(4242));
    assert!(app.session.prefs.connect.enabled);
    assert_eq!(app.session.prefs.connect.projects.len(), 1);
    assert!(saved.get() > 0, "it survives a restart");
    assert!(app.connect.link(dir.to_str().unwrap(), "x", Box::new(|| {})).is_ok());
    // Again: nothing duplicated.
    app.connect_project(&dir);
    assert_eq!(app.session.prefs.connect.projects.len(), 1);
    assert!(matches!(&app.ui.developer_connect_notice, Some(Ok(said)) if said.contains("already")));
}

#[test]
fn removing_a_project_ends_its_access_and_keeps_its_files() {
    let dir = folder("remove");
    let (mut app, _) = desk_app();
    app.connect_project(&dir);
    let ticket = app.connect.link(dir.to_str().unwrap(), "x", Box::new(|| {})).unwrap();
    app.connect_remove(dir.to_str().unwrap());
    assert!(!app.connect.linked(ticket.link));
    assert!(app.session.prefs.connect.projects.is_empty());
    assert!(dir.join(".mcp.json").exists());
    assert!(app.connect.link(dir.to_str().unwrap(), "x", Box::new(|| {})).is_err());
}

#[test]
fn turning_it_off_ends_every_link_and_is_remembered() {
    let dir = folder("off");
    let (mut app, _) = desk_app();
    app.connect_project(&dir);
    let ticket = app.connect.link(dir.to_str().unwrap(), "x", Box::new(|| {})).unwrap();
    app.connect_set_enabled(false);
    assert!(!app.connect.linked(ticket.link));
    assert_eq!(app.connect.port(), None);
    assert!(!app.session.prefs.connect.enabled);
}

#[test]
fn an_app_started_confined_refuses_to_grant() {
    let dir = folder("confined");
    let (mut app, _) = desk_app();
    app.automation_roots = Some(project_connect::project_roots(&dir).unwrap());
    app.connect_project(&dir);
    assert!(matches!(&app.ui.developer_connect_notice, Some(Err(why)) if why.contains("automation roots")));
    assert!(!dir.join(".mcp.json").exists());
    assert!(!app.connect_available());
}

#[test]
fn no_command_grants_a_folder() {
    // The only way in for an agent would be a command that changes `prefs.connect`.
    let dir = folder("no-command");
    let (mut app, _) = desk_app();
    let path = dir.to_str().unwrap();
    for c in figcraft_engine::all_commands().iter().filter(|c| c.id.starts_with("pref.") || c.id.starts_with("developer.")) {
        let _ = app.session.execute_as(Some("agent"), c.id, &json!({"path": path, "on": true, "projects": [path]}));
    }
    assert_eq!(app.session.prefs.connect, figcraft_engine::ConnectPrefs::default());
}

//! Developer Connect's live state, shared by the window and the desktop app's listener (thread
//! safe, no egui): the connected projects and the agents linked to them, and how the app is
//! reachable. Grants change only here and only from the window: no command touches them, so an
//! agent can't give itself a folder. See `docs/developer-integration/`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_engine::file_access::Roots;
use figcraft_engine::project_connect::{self, Change, Client};
use figcraft_engine::{ConnectedProject, Prefs};
use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{tr, trf};

/// A connected project: the folder a connection may read and write.
struct Grant {
    path: PathBuf,
    name: String,
    roots: Arc<Roots>,
}

/// One agent's connection.
struct Link {
    id: u64,
    project: PathBuf,
    client: String,
    /// Ends its socket.
    close: Box<dyn Fn() + Send>,
}

#[derive(Default)]
struct State {
    grants: Vec<Grant>,
    links: Vec<Link>,
    next_link: u64,
    /// The port Developer Connect listens on.
    port: Option<u16>,
    /// Why it isn't listening.
    problem: Option<String>,
    /// The `--control` channel: its port, or why it couldn't open.
    control: Option<Result<u16, String>>,
}

/// What a connection gets when it links to a project.
pub struct Ticket {
    pub link: u64,
    pub path: PathBuf,
    pub name: String,
    pub roots: Arc<Roots>,
}

#[derive(Default)]
pub struct Hub(Mutex<State>);

impl Hub {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The projects connected are `projects`: those that went lose their links at once.
    pub fn set_projects(&self, projects: &[ConnectedProject]) {
        let grants: Vec<Grant> = projects
            .iter()
            .filter_map(|p| {
                let path = std::fs::canonicalize(&p.path).ok()?;
                let roots = project_connect::project_roots(&path).ok()?;
                Some(Grant { path, name: p.name.clone(), roots })
            })
            .collect();
        let mut s = self.state();
        s.links.retain(|l| {
            let keep = grants.iter().any(|g| g.path == l.project);
            if !keep {
                (l.close)();
            }
            keep
        });
        s.grants = grants;
    }

    /// Link an agent of `client` to the project at `path`. `Err` is what it is told.
    pub fn link(&self, path: &str, client: &str, close: Box<dyn Fn() + Send>) -> Result<Ticket, String> {
        let not_connected = || format!("the project {path} isn't connected: open Developer Connect in FigCraft and connect it");
        let path = std::fs::canonicalize(path).map_err(|_| not_connected())?;
        let mut s = self.state();
        let grant = s.grants.iter().find(|g| g.path == path).ok_or_else(not_connected)?;
        let ticket = Ticket { link: s.next_link, path: grant.path.clone(), name: grant.name.clone(), roots: grant.roots.clone() };
        s.next_link += 1;
        s.links.push(Link { id: ticket.link, project: ticket.path.clone(), client: client.to_string(), close });
        Ok(ticket)
    }

    /// The connection `id` ended.
    pub fn unlink(&self, id: u64) {
        self.state().links.retain(|l| l.id != id);
    }

    /// Whether connection `id` may still act.
    pub fn linked(&self, id: u64) -> bool {
        self.state().links.iter().any(|l| l.id == id)
    }

    /// End every link: the app stops listening.
    pub fn close_links(&self) {
        for l in std::mem::take(&mut self.state().links) {
            (l.close)();
        }
    }

    pub fn set_listening(&self, port: Option<u16>, problem: Option<String>) {
        let mut s = self.state();
        (s.port, s.problem) = (port, problem);
    }

    pub fn set_control(&self, control: Result<u16, String>) {
        self.state().control = Some(control);
    }

    pub fn port(&self) -> Option<u16> {
        self.state().port
    }

    pub fn problem(&self) -> Option<String> {
        self.state().problem.clone()
    }

    pub fn control(&self) -> Option<Result<u16, String>> {
        self.state().control.clone()
    }

    /// The clients linked to the project at `path`, by name.
    pub fn clients(&self, path: &Path) -> Vec<String> {
        let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.state().links.iter().filter(|l| l.project == wanted).map(|l| l.client.clone()).collect()
    }

    /// For `ui.inspect`: counts and names only, an agent can read it.
    pub fn inspect(&self) -> Value {
        let s = self.state();
        json!({"listening": s.port.is_some(), "problem": s.problem, "projects": s.grants.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), "links": s.links.len()})
    }
}

/// What is said when the app was started confined.
fn confined_message() -> String {
    tr("FigCraft was started with automation roots, which keep to their own folders: start it without them to use Developer Connect.").to_string()
}

impl FigApp {
    /// Whether this app can listen for agents here.
    pub fn connect_available(&self) -> bool {
        self.services.connect_available() && self.automation_roots.is_none()
    }

    /// Listen as the preferences say, as the app starts.
    pub fn connect_resume(&mut self) {
        if self.session.prefs.connect.enabled {
            self.connect_listen();
        }
    }

    /// Start listening, with the preferences' projects.
    fn connect_listen(&mut self) {
        if self.automation_roots.is_some() {
            self.connect.set_listening(None, Some(confined_message()));
            return;
        }
        self.connect.set_projects(&self.session.prefs.connect.projects);
        match self.services.connect_start(self.connect.clone()) {
            Ok(port) => self.connect.set_listening(Some(port), None),
            Err(e) => self.connect.set_listening(None, Some(e)),
        }
    }

    fn connect_save(&mut self) {
        self.connect.set_projects(&self.session.prefs.connect.projects);
        let prefs: Prefs = self.session.prefs.clone();
        self.services.save_prefs(&prefs);
    }

    /// Turn listening on or off; off, every agent is disconnected.
    pub fn connect_set_enabled(&mut self, on: bool) {
        self.session.prefs.connect.enabled = on;
        if on {
            self.connect_listen();
        } else {
            self.connect.close_links();
            self.services.connect_stop();
            self.connect.set_listening(None, None);
        }
        self.connect_save();
    }

    /// Set the folder up for the client and connect it (again, for one already connected). What
    /// happened goes in the window's notice.
    pub fn connect_project(&mut self, path: &Path) {
        let clients = project_connect::suggested(path);
        let result = self.connect_setup(path, &clients);
        self.ui.developer_connect_notice = Some(result);
    }

    /// Set a connected project up for one more client.
    pub fn connect_client(&mut self, path: &Path, id: &str) {
        let result = match project_connect::client(id) {
            Some(client) => self.connect_setup(path, &[client]),
            None => Err(tr("No client to set up.").to_string()),
        };
        self.ui.developer_connect_notice = Some(result);
    }

    fn connect_setup(&mut self, path: &Path, clients: &[&Client]) -> Result<String, String> {
        if clients.is_empty() {
            return Err(tr("No client to set up.").to_string());
        }
        if self.automation_roots.is_some() {
            return Err(confined_message());
        }
        let entry = self
            .services
            .connect_cli()
            .ok_or_else(|| tr("figcraft-cli wasn't found, or is too old for Developer Connect: put the one from this release on PATH, or next to the FigCraft program, and connect again.").to_string())?;
        let mut said = Vec::new();
        for client in clients {
            let done = self.services.connect_setup(path, client, &entry)?;
            said.push(match done.client {
                Change::Added => trf("Added FigCraft to {file}.", &[("file", &client.config)]),
                Change::Updated => trf("Updated FigCraft's entry in {file}.", &[("file", &client.config)]),
                Change::Unchanged => trf("{file} already had FigCraft.", &[("file", &client.config)]),
            });
            said.extend(done.warnings);
        }
        let text = path.to_string_lossy().into_owned();
        let name = project_connect::status(path)
            .settings
            .name
            .unwrap_or_else(|| path.file_name().map_or(text.clone(), |n| n.to_string_lossy().into_owned()));
        let list = &mut self.session.prefs.connect;
        match list.projects.iter_mut().find(|p| p.path == text) {
            Some(p) => p.name = name,
            None => list.projects.push(ConnectedProject { path: text, name }),
        }
        list.enabled = true;
        self.connect_listen();
        self.connect_save();
        said.dedup();
        Ok(said.join("\n"))
    }

    /// Disconnect the project at `path`: its agents lose access at once. Its files stay.
    pub fn connect_remove(&mut self, path: &str) {
        self.session.prefs.connect.projects.retain(|p| p.path != path);
        self.ui.developer_connect_notice = None;
        self.connect_save();
    }
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;

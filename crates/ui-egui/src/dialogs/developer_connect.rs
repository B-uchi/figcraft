//! The Developer Connect window: whether the app is reachable, the local projects connected to it,
//! their configuration and the agents working in them. The only place that gives a folder to an
//! agent (`crate::connect`). See `docs/developer-integration/SETUP.md`.

use std::path::{Path, PathBuf};

use egui::{Context, Id, RichText, Ui};
use figcraft_engine::ConnectedProject;
use figcraft_engine::project_connect::{self, CLIENTS};
use serde_json::json;

use crate::FigApp;
use crate::i18n::{tr, trf, trn};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// What the user asked for this frame, acted on after drawing.
#[derive(Default)]
struct Clicks {
    close: bool,
    enabled: Option<bool>,
    add: bool,
    reconnect: Option<PathBuf>,
    /// Set a project up for one more client.
    client: Option<(PathBuf, &'static str)>,
    remove: Option<String>,
    asset_dir: Option<(PathBuf, String)>,
    /// Apply the project's mappings to the open design (or write them back).
    mappings: Option<(PathBuf, &'static str)>,
    copy: Option<String>,
}

/// Width of the rows' label column.
const LABEL_W: f32 = 150.0;
/// How often the window looks at its projects and links.
const REFRESH: f64 = 1.0;

pub fn show(app: &mut FigApp, ctx: &Context) {
    if !app.ui.developer_connect {
        return;
    }
    let t = Tokens::get(ctx);
    let mut open = true;
    let mut clicks = Clicks::default();
    super::window(tr("Developer Connect"), ctx).open(&mut open).show(ctx, |ui| {
        ui.set_width(460.0);
        body(app, ui, &t, &mut clicks);
    });
    crate::repaint_in(ctx, REFRESH);
    if !open || clicks.close {
        app.ui.developer_connect = false;
    }
    act(app, clicks);
}

fn act(app: &mut FigApp, clicks: Clicks) {
    if let Some(on) = clicks.enabled {
        app.connect_set_enabled(on);
    }
    if clicks.add {
        app.pick_folder(tr("Connect a project"), |app, dir| app.connect_project(&dir));
    }
    if let Some(path) = clicks.reconnect {
        app.connect_project(&path);
    }
    if let Some((path, id)) = clicks.client {
        app.connect_client(&path, id);
    }
    if let Some(path) = clicks.remove {
        app.connect_remove(&path);
    }
    if let Some((path, dir)) = clicks.asset_dir
        && let Err(e) = project_connect::set_asset_dir(&path, &dir)
    {
        app.show_error(e);
    }
    if let Some((path, command)) = clicks.mappings {
        let file = path.join(project_connect::CONNECT_FILE);
        if let Some(done) = app.run(command, json!({"path": file.to_string_lossy()})) {
            let n = done.get("linked").or_else(|| done.get("links")).and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
            app.toast(trn("{n} code mappings", n, &[]));
        }
    }
    if let Some(text) = clicks.copy {
        app.services.clipboard_set_text(&text);
        app.toast(tr("Copied").to_string());
    }
}

/// A line of secondary text.
fn note(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.text_secondary).font(regular(11.0)));
}

/// The label column of a row.
fn fact_label(ui: &mut Ui, t: &Tokens, label: &str) {
    let text = RichText::new(label).color(t.text_secondary).font(regular(11.0));
    ui.allocate_ui_with_layout(egui::vec2(LABEL_W, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(LABEL_W);
        ui.label(text);
    });
}

/// A label and its value on one row.
fn fact(ui: &mut Ui, t: &Tokens, label: &str, value: &str) {
    ui.horizontal(|ui| {
        fact_label(ui, t, label);
        ui.label(RichText::new(value).font(regular(11.0)));
    });
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(text).font(semibold(12.0)));
    ui.add_space(4.0);
}

fn body(app: &FigApp, ui: &mut Ui, t: &Tokens, clicks: &mut Clicks) {
    if !app.services.connect_available() {
        note(ui, t, tr("Developer Connect needs the desktop app."));
        clicks.close = widgets::button(ui, tr("Close"), true, true).clicked();
        return;
    }
    note(
        ui,
        t,
        tr(
            "Lets an AI coding agent on this computer read this design and work from it in a project you choose. FigCraft uploads nothing, but the agent's model sees what the agent reads.",
        ),
    );
    ui.add_space(8.0);
    let on = app.connect.port().is_some();
    if app.connect_available() && widgets::checkbox(ui, on, tr("Listen for connected projects")) {
        clicks.enabled = Some(!on);
    }
    status_rows(app, ui, t);
    heading(ui, tr("Projects"));
    let projects = app.session.prefs.connect.projects.clone();
    if projects.is_empty() {
        note(ui, t, tr("No project connected yet."));
    }
    egui::ScrollArea::vertical().id_salt("developer_connect_projects").max_height(300.0).show(ui, |ui| {
        for p in &projects {
            project_row(app, ui, t, p, clicks);
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        clicks.add = widgets::button(ui, tr("Connect a project…"), true, app.connect_available()).clicked();
        clicks.close = widgets::button(ui, tr("Close"), false, true).clicked();
    });
    match &app.ui.developer_connect_notice {
        Some(Ok(done)) => {
            ui.add_space(8.0);
            note(ui, t, done);
        }
        Some(Err(why)) => {
            ui.add_space(8.0);
            ui.label(RichText::new(why).color(t.danger).font(regular(11.0)));
        }
        None => {}
    }
}

/// How the app can be reached and what an agent would find.
fn status_rows(app: &FigApp, ui: &mut Ui, t: &Tokens) {
    let hub = &app.connect;
    let listening = hub.port().map_or_else(|| tr("Off").to_string(), |port| trf("On, local port {port}", &[("port", &port)]));
    fact(ui, t, tr("Developer Connect"), &listening);
    if let Some(why) = hub.problem() {
        ui.label(RichText::new(why).color(t.danger).font(regular(11.0)));
    }
    match hub.control() {
        Some(Ok(port)) => fact(ui, t, tr("Control channel"), &trf("Open to local programs on port {port}", &[("port", &port)])),
        Some(Err(why)) => fact(ui, t, tr("Control channel"), &trf("Didn't open: {why}", &[("why", &why)])),
        None => {}
    }
    let document = match app.session.ds() {
        Ok(d) => {
            let n = d.selection.len();
            format!("{} · {}", d.doc.name, trn("{n} layers selected", n, &[]))
        }
        Err(_) => tr("No document open").to_string(),
    };
    fact(ui, t, tr("Agents will see"), &document);
}

fn project_row(app: &FigApp, ui: &mut Ui, t: &Tokens, p: &ConnectedProject, clicks: &mut Clicks) {
    let path = Path::new(&p.path);
    let status = project_connect::status(path);
    let linked = app.connect.clients(path);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&p.name).font(semibold(12.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, tr("Disconnect"), false, true).clicked() {
                clicks.remove = Some(p.path.clone());
            }
            if widgets::button(ui, tr("Reconnect"), false, status.problem.is_none()).clicked() {
                clicks.reconnect = Some(path.to_path_buf());
            }
        });
    });
    note(ui, t, &p.path);
    if let Some(why) = &status.problem {
        ui.label(RichText::new(why).color(t.danger).font(regular(11.0)));
    }
    ui.horizontal(|ui| {
        fact_label(ui, t, tr("Configured for"));
        ui.spacing_mut().item_spacing.x = 4.0;
        for c in CLIENTS {
            let on = status.clients.contains(&c.label);
            let tip = if on { trf("{file} has FigCraft", &[("file", &c.config)]) } else { trf("Add FigCraft to {file}", &[("file", &c.config)]) };
            if widgets::button(ui, c.label, on, status.problem.is_none()).on_hover_text(tip).clicked() && !on {
                clicks.client = Some((path.to_path_buf(), c.id));
            }
        }
    });
    let agents = if linked.is_empty() { tr("None right now").to_string() } else { linked.join(", ") };
    fact(ui, t, tr("Agents connected"), &agents);
    ui.horizontal(|ui| {
        fact(ui, t, project_connect::CONNECT_FILE, &trn("{n} code mappings", status.settings.links, &[]));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let usable = status.connect_file && app.session.ds().is_ok();
            if widgets::button(ui, tr("Export"), false, usable).on_hover_text(tr("Write this design's mappings to the project")).clicked() {
                clicks.mappings = Some((path.to_path_buf(), "codeConnect.export"));
            }
            if widgets::button(ui, tr("Import"), false, usable).on_hover_text(tr("Apply the project's mappings to this design")).clicked() {
                clicks.mappings = Some((path.to_path_buf(), "codeConnect.import"));
            }
        });
    });
    ui.horizontal(|ui| {
        fact_label(ui, t, tr("Asset folder"));
        let id = Id::unique(("developer_connect_assets", &p.path));
        if let Some(dir) = widgets::text_field_named(ui, id, &status.settings.asset_dir, 200.0, tr("Asset folder")) {
            clicks.asset_dir = Some((path.to_path_buf(), dir));
        }
    });
    // The client starts the server itself; this is all that's left to do by hand.
    if let Some(client) = CLIENTS.first().filter(|c| linked.is_empty() && status.clients.contains(&c.label)) {
        note(ui, t, &trf("In the project, start {client} and approve the figcraft server:", &[("client", &client.label)]));
        let command = format!("cd {} && claude", shell_quoted(&p.path));
        ui.horizontal(|ui| {
            ui.label(RichText::new(&command).font(egui::FontId::monospace(11.0)));
            if widgets::button(ui, tr("Copy"), false, true).clicked() {
                clicks.copy = Some(command.clone());
            }
        });
    }
    if linked.is_empty() && status.clients.iter().any(|c| *c != "Claude Code") {
        note(ui, t, tr("In another editor, open the project folder and turn on the figcraft server in its MCP settings."));
    }
}

/// `path` as one shell word.
fn shell_quoted(path: &str) -> String {
    if path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
#[path = "developer_connect_tests.rs"]
mod tests;

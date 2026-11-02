//! `figcraft-cli connect setup` and `status`, and the folder `mcp --project` serves: the same
//! project code the app's Developer Connect window uses.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use figcraft_engine::project_connect::{self, CLIENTS, Change, McpEntry};

use crate::{CliError, CliResult};

/// The folder `dir` names, else the nearest upwards with a connect file, else here, resolved.
pub fn project_dir(dir: Option<&str>) -> CliResult<PathBuf> {
    let here = std::env::current_dir().map_err(|e| CliError::Failed(format!("the working folder: {e}")))?;
    let chosen = dir.map_or_else(|| project_connect::find_project(&here).unwrap_or_else(|| here.clone()), |d| here.join(d));
    std::fs::canonicalize(&chosen).map_err(|e| CliError::Failed(format!("{}: {e}", chosen.display())))
}

/// How a client starts this CLI.
fn entry() -> CliResult<McpEntry> {
    let exe = std::env::current_exe().ok();
    project_connect::cli_entry(
        exe.as_deref(),
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("APPIMAGE").as_deref(),
        &project_connect::understands_project,
    )
    .ok_or_else(|| CliError::Failed("can't tell how to start figcraft-cli here: put it on PATH".into()))
}

pub fn setup(dir: Option<&str>) -> CliResult<ExitCode> {
    let project = project_dir(dir)?;
    let client = CLIENTS.first().ok_or_else(|| CliError::Failed("no client to set up".into()))?;
    let done = project_connect::setup(&project, client, &entry()?, &project_connect::understands_project).map_err(CliError::Failed)?;
    let said = match done.client {
        Change::Added => "added FigCraft",
        Change::Updated => "updated FigCraft's entry",
        Change::Unchanged => "already has FigCraft",
    };
    println!("{}: {said}", client.config);
    println!("{}: {}", project_connect::CONNECT_FILE, if done.connect_file { "written" } else { "unchanged" });
    for w in &done.warnings {
        eprintln!("warning: {w}");
    }
    println!("Next: connect {} in FigCraft (Developer Connect, under the ? button); setup alone gives no access.", project.display());
    Ok(ExitCode::SUCCESS)
}

fn report(project: &Path) -> bool {
    let s = project_connect::status(project);
    println!("project:   {}", project.display());
    println!(
        "clients:   {}",
        if s.clients.is_empty() { "none configured (run `figcraft-cli connect setup`)".to_string() } else { s.clients.join(", ") }
    );
    println!("assets:    {}", s.settings.asset_dir);
    println!("mappings:  {}", s.settings.links);
    if let Some(p) = &s.problem {
        println!("problem:   {p}");
    }
    s.problem.is_none()
}

pub fn status(dir: Option<&str>) -> CliResult<ExitCode> {
    let project = project_dir(dir)?;
    let healthy = report(&project);
    match figcraft_mcp::connect::link(None, &project, "figcraft-cli connect status") {
        Ok(link) => {
            println!("app:       connected on 127.0.0.1:{}", link.port);
            Ok(if healthy { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Err(why) => {
            println!("app:       {why}");
            Ok(ExitCode::from(1))
        }
    }
}

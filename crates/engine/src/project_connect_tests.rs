//! Developer Connect's project configuration: merging into a user's files, setup and settings.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::file_access::{self, Access};
use crate::project_connect::*;

fn entry() -> McpEntry {
    McpEntry { command: "figcraft-cli".into(), args: vec!["mcp".into(), "--project".into()], machine_specific: false, typed: false }
}

fn client() -> &'static Client {
    &CLIENTS[0]
}

fn merge(existing: Option<&str>) -> Merged {
    merge_config(existing, client(), &entry(), &|_| true).unwrap()
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-project-connect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn servers(text: &str) -> Value {
    serde_json::from_str::<Value>(text).unwrap()["mcpServers"].clone()
}

#[test]
fn a_new_file_holds_the_entry_and_nothing_machine_specific() {
    let m = merge(None);
    assert_eq!(m.change, Change::Added);
    assert_eq!(servers(&m.text)["figcraft"], json!({"command": "figcraft-cli", "args": ["mcp", "--project"]}));
    assert!(m.text.ends_with("}\n"));
    assert_eq!(merge(Some("  \n")).change, Change::Added);
}

#[test]
fn other_servers_keep_every_byte_order_and_format() {
    // Not sorted, odd spacing, a key with an escaped quote and a brace in a string.
    let text = "{\n    \"zeta\": 1,\n    \"mcpServers\": {\n        \"zed\": {\"command\": \"z\", \"args\": [\"}\"]},\n        \"a\\\"b\": {\"command\": \"q\"}\n    },\n    \"alpha\": [1, {\"b\": 2}]\n}\n";
    let m = merge(Some(text));
    assert_eq!(m.change, Change::Added);
    let at = m.text.find(",\n        \"figcraft\"").expect("indented like its neighbours");
    let end = m.text.rfind("\n    },").unwrap();
    assert_eq!(format!("{}{}", &m.text[..at], &m.text[end..]), text);
    assert_eq!(servers(&m.text)["figcraft"]["command"], "figcraft-cli");
    assert_eq!(servers(&m.text)["zed"]["args"], json!(["}"]));
}

#[test]
fn merging_twice_changes_nothing() {
    let once = merge(Some("{\"mcpServers\": {\"other\": {\"command\": \"x\"}}}\n"));
    let twice = merge(Some(&once.text));
    assert_eq!(twice.change, Change::Unchanged);
    assert_eq!(twice.text, once.text);
    assert_eq!(servers(&once.text).as_object().unwrap().len(), 2, "no duplicate entry");
}

#[test]
fn a_file_without_servers_or_with_an_empty_list_gets_them() {
    for text in ["{}", "{ }", "{\"other\": true}", "{\n  \"mcpServers\": {}\n}\n", "{\"mcpServers\":{}}"] {
        let m = merge(Some(text));
        assert_eq!(servers(&m.text)["figcraft"]["args"], json!(["mcp", "--project"]), "{text}");
    }
    assert_eq!(serde_json::from_str::<Value>(&merge(Some("{\"other\": true}")).text).unwrap()["other"], true);
}

#[test]
fn an_entry_that_runs_something_else_is_updated_in_place() {
    let text = "{\"mcpServers\": {\"a\": {}, \"figcraft\": {\"command\": \"figcraft-cli\", \"args\": [\"mcp\", \"--attach\", \"7979\"]}, \"b\": {}}}";
    let m = merge(Some(text));
    assert_eq!(m.change, Change::Updated);
    assert_eq!(servers(&m.text)["figcraft"]["args"], json!(["mcp", "--project"]));
    let order: Vec<usize> = ["\"a\"", "\"figcraft\"", "\"b\""].iter().map(|k| m.text.find(k).unwrap()).collect();
    assert!(order.is_sorted(), "{}", m.text);
}

#[test]
fn a_teammates_entry_stays_when_it_runs_here_and_is_replaced_when_it_cant() {
    let text = "{\"mcpServers\": {\"figcraft\": {\"command\": \"/opt/figcraft-cli\", \"args\": [\"mcp\", \"--project\"]}}}";
    assert_eq!(merge_config(Some(text), client(), &entry(), &|_| true).unwrap().change, Change::Unchanged);
    let m = merge_config(Some(text), client(), &entry(), &|_| false).unwrap();
    assert_eq!((m.change, servers(&m.text)["figcraft"]["command"].as_str().unwrap()), (Change::Updated, "figcraft-cli"));
}

#[test]
fn an_appimage_entry_counts_as_the_project_server() {
    let text = "{\"mcpServers\": {\"figcraft\": {\"command\": \"/a/FigCraft.AppImage\", \"args\": [\"cli\", \"mcp\", \"--project\"]}}}";
    assert_eq!(merge(Some(text)).change, Change::Unchanged);
}

#[test]
fn a_second_server_running_the_cli_is_reported_not_removed() {
    let m = merge(Some("{\"mcpServers\": {\"design\": {\"command\": \"/x/figcraft-cli\", \"args\": [\"mcp\"]}}}"));
    assert!(m.warnings[0].contains("`design`"), "{:?}", m.warnings);
    assert!(servers(&m.text).get("design").is_some());
}

#[test]
fn files_it_cant_understand_are_refused() {
    for (text, says) in [("{nope", "valid JSON"), ("[1]", "isn't a JSON object"), ("{\"mcpServers\": 3}", "isn't an object")] {
        let e = merge_config(Some(text), client(), &entry(), &|_| true).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
}

#[test]
fn junk_never_panics_and_valid_results_keep_their_meaning() {
    let seeds = [
        "{\"mcpServers\":{\"a\":{\"args\":[\"\\\"\"]}}}",
        "{\"a\":[[[{}]]],\"mcpServers\":{}}",
        "{\"é\":\"日本\",\"mcpServers\":{\"日\":1}}",
        "{\"mcpServers\":{\"figcraft\":null}}",
    ];
    for seed in seeds {
        for cut in 0..seed.len() {
            let Some(text) = seed.get(..cut) else { continue };
            let _ = merge_config(Some(text), client(), &entry(), &|_| true);
        }
        let m = merge(Some(seed));
        let old: Value = serde_json::from_str(seed).unwrap();
        let new: Value = serde_json::from_str(&m.text).unwrap();
        for (k, v) in old["mcpServers"].as_object().unwrap() {
            if k != "figcraft" {
                assert_eq!(&new["mcpServers"][k], v);
            }
        }
    }
}

#[test]
fn setup_creates_both_files_and_is_idempotent() {
    let dir = project("setup");
    let first = setup(&dir, client(), &entry(), &|_| true).unwrap();
    assert_eq!((first.client, first.connect_file), (Change::Added, true));
    let mcp = std::fs::read(dir.join(".mcp.json")).unwrap();
    let connect = std::fs::read(dir.join(CONNECT_FILE)).unwrap();
    let second = setup(&dir, client(), &entry(), &|_| true).unwrap();
    assert_eq!((second.client, second.connect_file), (Change::Unchanged, false));
    assert_eq!(std::fs::read(dir.join(".mcp.json")).unwrap(), mcp);
    assert_eq!(std::fs::read(dir.join(CONNECT_FILE)).unwrap(), connect);
    let s = status(&dir);
    assert_eq!(s.clients, ["Claude Code"]);
    assert_eq!(s.settings.asset_dir, DEFAULT_ASSET_DIR);
    assert_eq!(s.problem, None);
    assert!(!String::from_utf8(mcp).unwrap().contains(dir.to_str().unwrap()), "no machine-specific path");
}

#[test]
fn setup_keeps_existing_mappings_and_adds_the_project_key() {
    let dir = project("mappings");
    let links = r#"{"version":1,"links":[{"component":"Button","language":"react","name":"Button","template":"<Button />"}]}"#;
    std::fs::write(dir.join(CONNECT_FILE), links).unwrap();
    assert!(setup(&dir, client(), &entry(), &|_| true).unwrap().connect_file);
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join(CONNECT_FILE)).unwrap()).unwrap();
    assert_eq!(v["links"][0]["component"], "Button");
    assert_eq!(v["project"]["assetDir"], DEFAULT_ASSET_DIR);
    assert_eq!(status(&dir).settings.links, 1);
}

#[test]
fn setup_leaves_an_invalid_file_alone_and_says_why() {
    let dir = project("invalid");
    std::fs::write(dir.join(".mcp.json"), "{broken").unwrap();
    assert!(setup(&dir, client(), &entry(), &|_| true).unwrap_err().contains("valid JSON"));
    assert_eq!(std::fs::read_to_string(dir.join(".mcp.json")).unwrap(), "{broken");
    assert!(!dir.join(CONNECT_FILE).exists());
}

#[cfg(unix)]
#[test]
fn setup_refuses_a_config_that_links_out_of_the_project() {
    let dir = project("link");
    let outside = project("link-outside");
    std::fs::write(outside.join("victim.json"), "{}").unwrap();
    std::os::unix::fs::symlink(outside.join("victim.json"), dir.join(".mcp.json")).unwrap();
    assert!(setup(&dir, client(), &entry(), &|_| true).unwrap_err().contains("outside"));
    assert_eq!(std::fs::read_to_string(outside.join("victim.json")).unwrap(), "{}");
}

#[test]
fn asset_folders_stay_inside_the_project() {
    for dir in ["public/figcraft", "assets", "./a/b"] {
        assert_eq!(check_asset_dir(dir), Ok(()), "{dir}");
    }
    for dir in ["", " ", "../up", "a/../../up", "/etc", "a\\b", "a\nb"] {
        assert!(check_asset_dir(dir).is_err(), "{dir:?}");
    }
    assert_eq!(Settings::parse(Some(r#"{"project":{"assetDir":"../../etc"}}"#)).asset_dir, DEFAULT_ASSET_DIR);
}

#[test]
fn set_asset_dir_edits_only_that_value() {
    let dir = project("assets");
    setup(&dir, client(), &entry(), &|_| true).unwrap();
    set_asset_dir(&dir, "static/design").unwrap();
    assert_eq!(status(&dir).settings.asset_dir, "static/design");
    assert!(set_asset_dir(&dir, "../out").is_err());
    assert_eq!(status(&dir).settings.asset_dir, "static/design");
}

#[cfg(unix)]
#[test]
fn set_asset_dir_refuses_a_folder_that_links_out() {
    let dir = project("asset-link");
    let outside = project("asset-outside");
    setup(&dir, client(), &entry(), &|_| true).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("out")).unwrap();
    assert!(set_asset_dir(&dir, "out/x").unwrap_err().contains("outside"));
}

#[test]
fn the_nearest_project_is_found_upwards() {
    let dir = project("find");
    setup(&dir, client(), &entry(), &|_| true).unwrap();
    let deep = dir.join("src/components");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(find_project(&deep).as_deref(), Some(dir.as_path()));
    assert_eq!(find_project(Path::new("/nonexistent/for/sure")), None);
}

#[test]
fn a_missing_folder_is_a_problem_not_a_failure() {
    let dir = project("gone");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(status(&dir).problem.unwrap().contains("gone"));
    assert!(setup(&dir, client(), &entry(), &|_| true).is_err());
}

#[test]
fn the_cli_is_found_on_path_else_beside_the_app() {
    let dir = project("cli");
    let name = format!("figcraft-cli{}", std::env::consts::EXE_SUFFIX);
    let app = dir.join("figcraft");
    let any = |_: &str| true;
    assert_eq!(cli_entry(Some(&app), None, None, &any), None, "nothing to find");
    std::fs::write(dir.join(&name), "").unwrap();
    let beside = cli_entry(Some(&app), None, None, &any).unwrap();
    assert!(beside.machine_specific && beside.command.ends_with(&name));
    let on_path = cli_entry(Some(Path::new("/elsewhere/figcraft")), Some(dir.as_os_str()), None, &any).unwrap();
    assert_eq!(on_path, entry());
    let image = cli_entry(None, None, Some(std::ffi::OsStr::new("/a/FigCraft.AppImage")), &any).unwrap();
    assert_eq!(image.args, ["cli", "mcp", "--project"]);
}

/// An older CLI installed on PATH has no `--project`: the one that came with the app is used.
#[test]
fn an_older_cli_on_path_is_passed_over() {
    let installed = project("cli-old");
    let (shipped, app) = (project("cli-new"), PathBuf::from("figcraft"));
    let name = format!("figcraft-cli{}", std::env::consts::EXE_SUFFIX);
    std::fs::write(installed.join(&name), "").unwrap();
    std::fs::write(shipped.join(&name), "").unwrap();
    let new = shipped.join(&name).to_str().unwrap().to_string();
    let usable = |c: &str| c == new;
    let found = cli_entry(Some(&shipped.join(&app)), Some(installed.as_os_str()), None, &usable).unwrap();
    assert_eq!((found.command.as_str(), found.machine_specific), (new.as_str(), true));
    assert_eq!(cli_entry(Some(&shipped.join(&app)), Some(installed.as_os_str()), None, &|_| false), None);
}

/// A committed entry whose command can't run the project server is replaced.
#[test]
fn a_committed_entry_for_an_older_cli_is_replaced() {
    let text = r#"{"mcpServers": {"figcraft": {"command": "figcraft-cli", "args": ["mcp", "--project"]}}}"#;
    let wanted = McpEntry { command: "/new/figcraft-cli".into(), ..entry() };
    let m = merge_config(Some(text), client(), &wanted, &|c| c != "figcraft-cli").unwrap();
    assert_eq!((m.change, servers(&m.text)["figcraft"]["command"].as_str().unwrap()), (Change::Updated, "/new/figcraft-cli"));
}

#[cfg(unix)]
#[test]
fn understanding_the_flag_is_asked_of_the_program() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = project("understands");
    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().to_string()
    };
    assert!(understands_project(&script("new", "echo '  --project [DIR]'")));
    assert!(!understands_project(&script("old", "echo '  --attach PORT'")));
    assert!(!understands_project(&script("stuck", "sleep 30")), "doesn't hold the window up");
    assert!(!understands_project("/no/such/program"));
}

#[test]
fn export_keeps_the_keys_it_doesnt_own() {
    let dir = project("export");
    setup(&dir, client(), &entry(), &|_| true).unwrap();
    set_asset_dir(&dir, "static").unwrap();
    let mut s = crate::Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let path = dir.join(CONNECT_FILE);
    s.execute("codeConnect.export", &json!({"path": path.to_str().unwrap()})).unwrap();
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((v["project"]["assetDir"].as_str(), v["version"].as_u64(), v["links"].as_array().map(Vec::len)), (Some("static"), Some(1), Some(0)));
    // Without a path the text is the plain mappings, as it always was.
    let text = s.execute("codeConnect.export", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(!text.contains("project"));
}

#[test]
fn project_roots_hold_the_project_only() {
    let dir = project("roots");
    let roots = project_roots(&dir).unwrap();
    assert!(roots.check(&dir.join("a.png"), Access::Write).is_ok());
    assert!(roots.check(&dir.join("../a.png"), Access::Write).is_err());
    assert!(!file_access::confined(Some(&roots), || file_access::allowed("/etc/hostname", Access::Read)));
}

#[test]
fn instance_files_round_trip() {
    let i = Instance { version: DISCOVERY_VERSION, pid: 1, port: 2, token: "t".into(), app_version: "0".into() };
    let text = serde_json::to_string(&i).unwrap();
    assert_eq!(serde_json::from_str::<Instance>(&text).unwrap(), i);
}

fn named(id: &str) -> &'static Client {
    crate::project_connect::client(id).unwrap()
}

#[test]
fn each_client_gets_its_own_file_key_and_arguments() {
    let dir = project("clients");
    for c in CLIENTS {
        setup(&dir, c, &entry(), &|_| true).unwrap();
    }
    let read = |f: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap();
    assert_eq!(read(".mcp.json")["mcpServers"]["figcraft"]["args"], json!(["mcp", "--project"]));
    assert_eq!(read(".cursor/mcp.json")["mcpServers"]["figcraft"]["args"], json!(["mcp", "--project", "${workspaceFolder}"]));
    let vscode = &read(".vscode/mcp.json")["servers"]["figcraft"];
    assert_eq!((vscode["type"].as_str(), vscode["command"].as_str()), (Some("stdio"), Some("figcraft-cli")));
    assert_eq!(vscode["args"], json!(["mcp", "--project", "${workspaceFolder}"]));
    assert_eq!(read(".gemini/settings.json")["mcpServers"]["figcraft"]["args"], json!(["mcp", "--project"]));
    assert_eq!(status(&dir).clients, ["Claude Code", "Cursor", "VS Code", "Gemini CLI"]);
    for c in CLIENTS {
        assert_eq!(setup(&dir, c, &entry(), &|_| true).unwrap().client, Change::Unchanged, "{}", c.id);
    }
}

#[test]
fn a_clients_other_settings_stay() {
    let dir = project("gemini-settings");
    std::fs::create_dir_all(dir.join(".gemini")).unwrap();
    let before = "{\n  \"theme\": \"Dracula\",\n  \"mcpServers\": {\n    \"git\": {\"command\": \"git-mcp\"}\n  }\n}\n";
    std::fs::write(dir.join(".gemini/settings.json"), before).unwrap();
    setup(&dir, named("gemini-cli"), &entry(), &|_| true).unwrap();
    let after = std::fs::read_to_string(dir.join(".gemini/settings.json")).unwrap();
    assert!(after.starts_with("{\n  \"theme\": \"Dracula\",\n  \"mcpServers\": {\n    \"git\": {\"command\": \"git-mcp\"},"), "{after}");
}

#[test]
fn only_clients_a_project_uses_are_suggested() {
    let dir = project("suggest");
    let ids = |d: &Path| suggested(d).iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(&dir), ["claude-code"]);
    std::fs::create_dir_all(dir.join(".vscode")).unwrap();
    std::fs::create_dir_all(dir.join(".cursor")).unwrap();
    assert_eq!(ids(&dir), ["claude-code", "cursor", "vscode"]);
}

#[test]
fn an_entry_for_another_folder_still_counts_as_ours() {
    let text = r#"{"servers": {"figcraft": {"type": "stdio", "command": "figcraft-cli", "args": ["mcp", "--project", "${workspaceFolder}"]}}}"#;
    assert_eq!(merge_config(Some(text), named("vscode"), &entry().for_client(named("vscode")), &|_| true).unwrap().change, Change::Unchanged);
}

//! Responsive context holds only what the designer stated.

use serde_json::{Value, json};

use crate::Session;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn frame(s: &mut Session, name: &str, width: u32) -> u64 {
    s.execute("node.create", &json!({"type": "frame", "name": name, "width": width, "height": 800})).unwrap()["id"].as_u64().unwrap()
}

fn context(s: &mut Session, id: u64) -> Value {
    s.execute("inspect.designContext", &json!({"ids": [id]})).unwrap()
}

#[test]
fn nothing_is_inferred_from_names_or_sizes() {
    let mut s = session();
    let phone = frame(&mut s, "Home / Mobile", 390);
    frame(&mut s, "Home / Desktop", 1440);
    let v = context(&mut s, phone);
    assert_eq!(v["responsive"], json!({"explicit": false, "versions": [], "rules": []}));
    assert!(v["missing"].as_array().unwrap().iter().any(|m| m["what"] == "responsive behaviour"));
}

#[test]
fn marked_frames_are_told_apart_and_come_with_their_alternates() {
    let mut s = session();
    let phone = frame(&mut s, "Home phone", 390);
    let tablet = frame(&mut s, "Home tablet", 768);
    let desk = frame(&mut s, "Home large", 1440);
    let other = frame(&mut s, "About", 1440);
    for (id, viewport) in [(desk, "desktop"), (phone, "mobile"), (tablet, "tablet")] {
        s.execute("responsive.set", &json!({"id": id, "viewport": viewport, "group": "Home"})).unwrap();
    }
    s.execute("responsive.set", &json!({"id": other, "viewport": "desktop"})).unwrap();
    let v = context(&mut s, desk);
    assert_eq!(v["responsive"]["explicit"], true);
    let versions = v["responsive"]["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 1, "other designs stay out: {versions:?}");
    let frames: Vec<&str> = versions[0]["frames"].as_array().unwrap().iter().map(|f| f["viewport"].as_str().unwrap()).collect();
    assert_eq!(frames, ["mobile", "tablet", "desktop"], "narrowest first");
    assert!(v["missing"].as_array().unwrap().iter().any(|m| m["what"] == "breakpoints"));
    let all = s.execute("responsive.list", &json!({})).unwrap();
    assert_eq!(all["versions"].as_array().unwrap().len(), 2);
}

#[test]
fn a_rule_is_the_designers_words_in_their_category() {
    let mut s = session();
    let f = frame(&mut s, "Home", 1440);
    let side = s.execute("node.create", &json!({"type": "frame", "parent": f, "name": "Sidebar", "width": 240, "height": 800})).unwrap()["id"]
        .as_u64()
        .unwrap();
    s.execute("responsive.rule", &json!({"id": side, "text": "Sidebar becomes a drawer below 768 px"})).unwrap();
    s.execute("annotation.add", &json!({"id": side, "label": "Ordinary note"})).unwrap();
    let v = context(&mut s, f);
    let rules = v["responsive"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!((rules[0]["layer"].as_str(), rules[0]["text"].as_str()), (Some("Sidebar"), Some("Sidebar becomes a drawer below 768 px")));
    let kept = s.execute("annotation.categories", &json!({})).unwrap();
    assert_eq!(kept["categories"].as_array().unwrap().iter().filter(|c| c["name"] == "Responsive").count(), 1);
    s.execute("responsive.rule", &json!({"id": f, "text": "Second"})).unwrap();
    let kept = s.execute("annotation.categories", &json!({})).unwrap();
    assert_eq!(kept["categories"].as_array().unwrap().iter().filter(|c| c["name"] == "Responsive").count(), 1, "not made twice");
}

#[test]
fn a_rule_on_the_frame_reaches_a_layer_read_on_its_own() {
    let mut s = session();
    let f = frame(&mut s, "Home", 1440);
    let child = s.execute("node.create", &json!({"type": "rectangle", "parent": f, "width": 10, "height": 10})).unwrap()["id"].as_u64().unwrap();
    s.execute("responsive.rule", &json!({"id": f, "text": "Cards stack on mobile"})).unwrap();
    assert_eq!(context(&mut s, child)["responsive"]["rules"][0]["text"], "Cards stack on mobile");
}

#[test]
fn marking_is_undoable_saved_and_clearable() {
    let mut s = session();
    let f = frame(&mut s, "Home", 1440);
    s.execute("responsive.set", &json!({"id": f, "viewport": "desktop", "group": "Home"})).unwrap();
    let path = std::env::temp_dir().join(format!("figcraft-responsive-{}.figcraft", std::process::id()));
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut back = Session::new();
    back.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(crate::responsive::groups(back.doc().unwrap()).len(), 1, "it survives saving");
    let _ = std::fs::remove_file(&path);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(crate::responsive::groups(s.doc().unwrap()).is_empty());
    s.execute("responsive.set", &json!({"id": f, "viewport": "mobile"})).unwrap();
    s.execute("responsive.set", &json!({"id": f, "viewport": null})).unwrap();
    assert!(crate::responsive::groups(s.doc().unwrap()).is_empty());
}

#[test]
fn bad_input_is_refused() {
    let mut s = session();
    let f = frame(&mut s, "Home", 1440);
    let r = s.execute("node.create", &json!({"type": "rectangle", "width": 10, "height": 10})).unwrap()["id"].as_u64().unwrap();
    for p in [
        json!({"id": f}),
        json!({"id": f, "viewport": "watch"}),
        json!({"id": r, "viewport": "mobile"}),
        json!({"id": f, "viewport": "mobile", "group": "  "}),
    ] {
        assert!(s.execute("responsive.set", &p).is_err(), "{p}");
    }
    assert!(s.execute("responsive.rule", &json!({"id": f, "text": " "})).is_err());
    assert!(s.execute("responsive.rule", &json!({"id": f})).is_err());
}

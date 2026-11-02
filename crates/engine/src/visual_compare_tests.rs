//! `inspect.compare` on captures made from the design itself, then changed the way a build can be
//! wrong, so the expected answer is known.

use base64::Engine as _;
use serde_json::{Value, json};

use crate::Session;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn make(s: &mut Session, params: Value) -> u64 {
    s.execute("node.create", &params).unwrap()["id"].as_u64().unwrap()
}

/// A white 400 × 300 frame with a blue box.
fn card(s: &mut Session) -> (u64, u64) {
    let f = make(s, json!({"type": "frame", "name": "Card", "width": 400, "height": 300, "props": {"fill": "#FFFFFF"}}));
    let b = make(
        s,
        json!({"type": "rectangle", "parent": f, "name": "Box", "x": 100, "y": 80, "width": 120, "height": 60, "props": {"fill": "#2040C0"}}),
    );
    (f, b)
}

/// The frame as a PNG, as a browser capture would be, at `scale`.
fn capture(s: &mut Session, id: u64, scale: f64) -> String {
    let v = s.execute("file.export", &json!({"ids": [id], "format": "png", "scale": scale})).unwrap();
    v["data"].as_str().unwrap().to_string()
}

fn compare(s: &mut Session, id: u64, data: &str) -> Value {
    s.execute("inspect.compare", &json!({"id": id, "data": data})).unwrap()
}

fn differs<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v["differences"].as_array().unwrap().iter().find(|d| d["name"] == name)
}

#[test]
fn a_faithful_capture_has_no_differences() {
    let mut s = session();
    let (f, _) = card(&mut s);
    let data = capture(&mut s, f, 1.0);
    let v = compare(&mut s, f, &data);
    assert_eq!(v["total"], 0, "{v}");
    assert_eq!(v["changedPercent"], 0.0);
    assert_eq!(v["viewport"], json!({"width": 400.0, "height": 300.0}));
    assert_eq!(v["capture"]["sameSize"], true);
}

#[test]
fn a_retina_capture_is_the_same_design() {
    let mut s = session();
    let (f, _) = card(&mut s);
    let data = capture(&mut s, f, 2.0);
    let v = compare(&mut s, f, &data);
    assert_eq!(v["capture"]["density"], 2.0);
    assert_eq!(v["total"], 0, "{v}");
}

#[test]
fn a_wrong_colour_names_the_layer_and_both_colours() {
    let mut s = session();
    let (f, b) = card(&mut s);
    s.execute("node.set", &json!({"ids": [b], "props": {"fill": "#C02020"}})).unwrap();
    let data = capture(&mut s, f, 1.0);
    s.execute("node.set", &json!({"ids": [b], "props": {"fill": "#2040C0"}})).unwrap();
    let v = compare(&mut s, f, &data);
    let d = differs(&v, "Box").unwrap_or_else(|| panic!("{v}"));
    assert_eq!(d["kind"], "color");
    assert_eq!((d["expected"].as_str(), d["actual"].as_str()), (Some("#2040C0"), Some("#C02020")));
    assert!(differs(&v, "Card").is_none(), "reported once, on the deepest layer: {v}");
}

#[test]
fn a_moved_layer_says_how_far() {
    let mut s = session();
    let (f, b) = card(&mut s);
    s.execute("node.set", &json!({"ids": [b], "props": {"x": 108, "y": 76}})).unwrap();
    let data = capture(&mut s, f, 1.0);
    s.execute("node.set", &json!({"ids": [b], "props": {"x": 100, "y": 80}})).unwrap();
    let v = compare(&mut s, f, &data);
    let d = differs(&v, "Box").unwrap_or_else(|| panic!("{v}"));
    assert_eq!(d["kind"], "position");
    assert_eq!(d["offset"], json!({"x": 8.0, "y": -4.0}));
}

#[test]
fn a_capture_of_another_size_says_so() {
    let mut s = session();
    let (f, _) = card(&mut s);
    let wider = make(&mut s, json!({"type": "frame", "name": "Wide", "width": 400, "height": 500, "props": {"fill": "#FFFFFF"}}));
    let data = capture(&mut s, wider, 1.0);
    let v = compare(&mut s, f, &data);
    assert_eq!(v["capture"]["sameSize"], false);
    assert_eq!(v["capture"]["cssHeight"], 500.0);
}

#[test]
fn hidden_layers_arent_blamed() {
    let mut s = session();
    let (f, b) = card(&mut s);
    let data = capture(&mut s, f, 1.0);
    s.execute("node.set", &json!({"ids": [b], "props": {"visible": false}})).unwrap();
    let v = compare(&mut s, f, &data);
    assert!(differs(&v, "Box").is_none());
    assert!(differs(&v, "Card").is_some(), "the frame shows what it didn't expect: {v}");
}

#[test]
fn a_path_stays_inside_a_confined_project() {
    let mut s = session();
    let (f, _) = card(&mut s);
    let dir = std::env::temp_dir().join(format!("figcraft-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let inside = std::fs::canonicalize(&dir).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(capture(&mut s, f, 1.0)).unwrap();
    std::fs::write(inside.join("shot.png"), bytes).unwrap();
    assert_eq!(s.execute("inspect.compare", &json!({"id": f, "path": inside.join("shot.png")})).unwrap()["total"], 0);
    let roots = crate::project_connect::project_roots(&inside.join("nope")).err();
    assert!(roots.is_some());
    let sub = inside.join("project");
    std::fs::create_dir_all(&sub).unwrap();
    let roots = crate::project_connect::project_roots(&sub).unwrap();
    let refused = crate::file_access::confined(Some(&roots), || s.execute("inspect.compare", &json!({"id": f, "path": inside.join("shot.png")})));
    assert!(refused.unwrap_err().to_string().contains("automation path rejected"));
}

#[test]
fn junk_is_refused_without_a_panic() {
    let mut s = session();
    let (f, _) = card(&mut s);
    for p in [
        json!({"id": f}),
        json!({"id": f, "data": "not base64!"}),
        json!({"id": f, "data": "aGVsbG8="}),
        json!({"id": 999_999, "data": capture(&mut s, f, 1.0)}),
    ] {
        assert!(s.execute("inspect.compare", &p).is_err(), "{p}");
    }
    let data = capture(&mut s, f, 1.0);
    assert!(s.execute("inspect.compare", &json!({"id": f, "data": data, "tolerance": "x", "density": -1})).is_ok());
}

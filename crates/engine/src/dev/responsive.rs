//! Responsive context: what the designer states about other screen sizes. A file has no
//! breakpoints, so this holds only what was said outright: frames marked as the mobile, tablet or
//! desktop version of one design (in layer data, so the file format doesn't change), and notes in
//! the Responsive annotation category. Nothing here is worked out from names or sizes: an agent
//! must never take a guess for the designer's word.

use figcraft_doc::{Annotation, AnnotationCategory, Document, NodeId, NodeKind};
use serde_json::{Value, json};

use crate::annotations::{categories, category_of, find_category, free_category_id, unused_color};
use crate::cmd::CommandSpec;
use crate::params::{get, string, targets};
use crate::{Result, Session, bad, spec};

/// Layer data key: which version of a design a frame is.
pub const VIEWPORT_KEY: &str = "figcraft.viewport";
/// Layer data key: which design it is a version of.
pub const GROUP_KEY: &str = "figcraft.viewportGroup";
/// The annotation category responsive rules go in.
pub const RULES_CATEGORY: &str = "Responsive";
/// The versions a frame can be.
pub const VIEWPORTS: [&str; 3] = ["mobile", "tablet", "desktop"];
/// Longest group name (chars).
const MAX_GROUP: usize = 100;
/// Most layers looked through for rules.
const MAX_WALK: usize = 50_000;

/// A marked frame as JSON.
fn frame_json(doc: &Document, id: NodeId) -> Option<Value> {
    let n = doc.node(id)?;
    Some(
        json!({"id": id, "name": n.name, "viewport": n.plugin_data.get(VIEWPORT_KEY)?, "width": n.width, "height": n.height, "page": doc.page_of(id)}),
    )
}

/// Every design with marked frames, narrowest first.
pub fn groups(doc: &Document) -> Vec<(String, Vec<NodeId>)> {
    let mut out: std::collections::BTreeMap<String, Vec<NodeId>> = std::collections::BTreeMap::new();
    for n in doc.nodes().filter(|n| n.plugin_data.contains_key(VIEWPORT_KEY)) {
        let group = n.plugin_data.get(GROUP_KEY).cloned().unwrap_or_else(|| n.name.clone());
        out.entry(group).or_default().push(n.id);
    }
    for frames in out.values_mut() {
        frames.sort_by(|a, b| doc.node(*a).map_or(0.0, |n| n.width).total_cmp(&doc.node(*b).map_or(0.0, |n| n.width)));
    }
    out.into_iter().collect()
}

fn group_json(doc: &Document, group: &str, frames: &[NodeId]) -> Value {
    json!({"group": group, "frames": frames.iter().filter_map(|f| frame_json(doc, *f)).collect::<Vec<_>>()})
}

/// Whether `a` is a responsive rule.
fn is_rule(doc: &Document, a: &Annotation) -> bool {
    category_of(doc, a).is_some_and(|c| c.name.eq_ignore_ascii_case(RULES_CATEGORY))
}

/// `ids`, the layers above them and those under them.
fn around(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for id in ids {
        let mut at = doc.node(*id).and_then(|n| n.parent);
        while let Some(p) = at {
            out.push(p);
            at = doc.node(p).and_then(|n| n.parent);
        }
        out.extend(doc.subtree(*id));
        if out.len() > MAX_WALK {
            out.truncate(MAX_WALK);
            break;
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// What the designer states about `ids` and the design they belong to.
pub fn context(doc: &Document, ids: &[NodeId]) -> Value {
    let near = around(doc, ids);
    let rules: Vec<Value> = near
        .iter()
        .filter_map(|i| doc.node(*i).map(|n| (i, n)))
        .flat_map(|(i, n)| n.annotations.iter().filter(|a| is_rule(doc, a)).map(move |a| json!({"id": i, "layer": n.name, "text": a.label})))
        .collect();
    let versions: Vec<Value> = groups(doc)
        .iter()
        .filter(|(_, frames)| frames.iter().any(|f| near.contains(f)))
        .map(|(group, frames)| group_json(doc, group, frames))
        .collect();
    json!({"explicit": !rules.is_empty() || !versions.is_empty(), "versions": versions, "rules": rules})
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "responsive.set";
    let ids = targets(s, p);
    if ids.is_empty() {
        return Err(bad(cmd, "select a frame"));
    }
    // `get` reads an explicit null as left out; here it means clear.
    let viewport = match p.get("viewport") {
        None => return Err(bad(cmd, "`viewport` required: mobile|tablet|desktop, or null to clear")),
        Some(Value::Null) => None,
        Some(Value::String(v)) if VIEWPORTS.contains(&v.as_str()) => Some(v.clone()),
        Some(_) => return Err(bad(cmd, "viewport: mobile|tablet|desktop, or null to clear")),
    };
    let group = string(p, "group").map(|g| g.trim().to_string());
    if group.as_ref().is_some_and(|g| g.is_empty() || g.chars().count() > MAX_GROUP) {
        return Err(bad(cmd, format!("group: a name up to {MAX_GROUP} characters")));
    }
    let doc = s.doc_mut()?;
    for id in &ids {
        let n = doc.get(*id)?;
        if !(n.kind.is_frame_like() || matches!(n.kind, NodeKind::Section { .. })) {
            return Err(bad(cmd, format!("{id} isn't a frame")));
        }
    }
    for id in &ids {
        let n = doc.get_mut(*id)?;
        match &viewport {
            Some(v) => {
                n.plugin_data.insert(VIEWPORT_KEY.into(), v.clone());
                if let Some(g) = &group {
                    n.plugin_data.insert(GROUP_KEY.into(), g.clone());
                }
            }
            None => {
                n.plugin_data.remove(VIEWPORT_KEY);
                n.plugin_data.remove(GROUP_KEY);
            }
        }
    }
    Ok(json!({"ids": ids, "viewport": viewport}))
}

fn rule(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "responsive.rule";
    let id = targets(s, p).first().copied().ok_or_else(|| bad(cmd, "select a layer"))?;
    let text = string(p, "text").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).ok_or_else(|| bad(cmd, "`text` required"))?;
    let doc = s.doc_mut()?;
    doc.get(id)?;
    let mut list = categories(doc).to_vec();
    let category = match find_category(&list, RULES_CATEGORY) {
        Some(c) => c.id.clone(),
        None => {
            let (cid, color) = (free_category_id(&list), unused_color(&list));
            list.push(AnnotationCategory { id: cid.clone(), name: RULES_CATEGORY.into(), color });
            doc.annotation_categories = Some(list);
            cid
        }
    };
    let n = doc.get_mut(id)?;
    n.annotations.push(Annotation { label: text, properties: Vec::new(), category: Some(category) });
    Ok(json!({"id": id, "index": n.annotations.len() - 1}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "responsive.set",
            "Mark as screen size version",
            "{ids? (frames; default the selection), viewport: mobile|tablet|desktop (null: clear), group? (the design they are versions of; default each frame's name)}  says which version of a design a frame is, in its layer data",
            true,
            set
        ),
        spec!(
            "responsive.rule",
            "Add responsive rule",
            "{id? (default the first selected layer), text (e.g. \"Sidebar becomes a drawer below 768 px\")}  a note in the Responsive annotation category, made if the file lacks it",
            true,
            rule
        ),
        spec!(
            "responsive.list",
            "List screen size versions",
            "{ids? | id? (what is stated about those layers; default every marked frame)}  only what the designer stated, nothing inferred  → {versions: [{group, frames: [{id, name, viewport, width, height, page}]}], rules?: [{id, layer, text}], explicit?}",
            false,
            |s, p| {
                let doc = s.doc()?;
                if get(p, "ids").is_some() || get(p, "id").is_some() {
                    return Ok(context(doc, &targets(s, p)));
                }
                Ok(json!({"versions": groups(doc).iter().map(|(g, f)| group_json(doc, g, f)).collect::<Vec<_>>()}))
            }
        ),
    ]
}

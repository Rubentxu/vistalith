//! Excalidraw semantic bindings (SPK-009, ADR-014, VISUAL-THINKING.md).
//!
//! The renderer may renumber shapes at any time (copy-paste, export/import,
//! regeneration), so bindings live OUTSIDE the canvas: each binding is a
//! durable advisory subject (`vistalith:sketch-element`) linked to its
//! semantic subject with `visualizes`. The Excalidraw shape id is recorded
//! as provenance only; the stable key is a content fingerprint (shape
//! type plus normalized text), and `customData { vistalith: "ns:kind:id" }`
//! carries the identity inside the scene file itself (the same pattern as
//! the LikeC4 metadata round-trip).
//!
//! Import is explicit and additive: it binds shapes to EXISTING subjects,
//! re-binds by fingerprint when ids changed, and (only when asked) creates
//! canvas note primitives for unbound text shapes — progressive
//! formalization step 1. It never guesses under ambiguity.

use serde::Serialize;
use sha2::{Digest, Sha256};
use vistalith_domain::{
    Actor, AuthorityClass, CanvasBound, CanvasGeometry, EventPayload, Namespace, Provenance,
    SubjectKind, SubjectRef, VEvent,
};

use crate::graph::SemanticWorldGraph;
use crate::store::{GraphStore, StoreError};

/// Scene name used when the caller does not care (single-canvas UIs).
pub const DEFAULT_SCENE: &str = "default";

pub const VIA_CUSTOM_DATA: &str = "custom-data";
pub const VIA_FINGERPRINT: &str = "fingerprint";
pub const VIA_IMPORT: &str = "import";

const CANVAS_PRIMITIVE_KINDS: [SubjectKind; 4] = [
    SubjectKind::Note,
    SubjectKind::Question,
    SubjectKind::Hypothesis,
    SubjectKind::Option,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExcalidrawError {
    Parse(String),
    InvalidIdentity(String),
    Store(String),
}

impl std::fmt::Display for ExcalidrawError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExcalidrawError::Parse(message) => write!(f, "excalidraw scene error: {message}"),
            ExcalidrawError::InvalidIdentity(raw) => {
                write!(f, "invalid vistalith identity `{raw}`")
            }
            ExcalidrawError::Store(message) => write!(f, "store error during import: {message}"),
        }
    }
}

impl std::error::Error for ExcalidrawError {}

/// How import may behave for shapes it cannot bind on its own.
#[derive(Debug, Clone, PartialEq)]
pub struct ExcalidrawImportOptions {
    /// Canvas scene name (bindings are namespaced per scene).
    pub scene: String,
    /// Create canvas `note` primitives for unbound TEXT shapes (explicit
    /// progressive formalization; default off — import never invents
    /// semantics silently).
    pub create_missing: bool,
}

impl Default for ExcalidrawImportOptions {
    fn default() -> Self {
        ExcalidrawImportOptions {
            scene: DEFAULT_SCENE.to_owned(),
            create_missing: false,
        }
    }
}

/// What the import did, element by element — explicit over silent.
/// Bindings are content-keyed per scene (scene + fingerprint + subject):
/// shape ids changing never produces new bindings, so re-importing a scene
/// — with ids renumbered, or even with `customData` stripped — is a no-op
/// as long as the text stays the same.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ExcalidrawImportReport {
    /// Subjects bound through `customData.vistalith`.
    pub bound: Vec<SubjectRef>,
    /// New canvas note primitives created (`create_missing`).
    pub created_primitives: Vec<SubjectRef>,
    /// Bindings that already existed (idempotent re-import).
    pub skipped_bindings: Vec<SubjectRef>,
    /// `customData.vistalith` refs that do not exist in the graph.
    pub unknown_subjects: Vec<String>,
    /// Shapes left unbound (no customData, no unique fingerprint match).
    pub unbound_elements: Vec<String>,
}

/// A stored binding as seen from the outside (read model).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CanvasBinding {
    pub binding: SubjectRef,
    pub subject: SubjectRef,
    pub scene: String,
    pub shape_id: String,
    pub fingerprint: String,
    pub via: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<CanvasGeometry>,
}

/// Stable content fingerprint: shape type + normalized text. Moving a shape
/// or renumbering it never changes this; editing its text does (that is a
/// different shape, semantically).
pub fn fingerprint(shape_type: &str, text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_space = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !previous_space {
                normalized.push(' ');
                previous_space = true;
            }
        } else {
            normalized.extend(ch.to_lowercase());
            previous_space = false;
        }
    }
    let normalized = normalized.trim_end().to_owned();
    let digest = Sha256::digest(format!("{shape_type}\u{0}{normalized}").as_bytes());
    let hex: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    hex
}

struct ParsedElement {
    shape_id: String,
    shape_type: String,
    text: String,
    geometry: Option<CanvasGeometry>,
    identity: Option<String>,
}

fn parse_element(value: &serde_json::Value) -> Result<ParsedElement, ExcalidrawError> {
    let shape_id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExcalidrawError::Parse("element without `id`".to_owned()))?
        .to_owned();
    let shape_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("rectangle")
        .to_owned();
    let text = value
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    let number = |key: &str| value.get(key).and_then(|v| v.as_f64());
    let geometry = match (number("x"), number("y"), number("width"), number("height")) {
        (Some(x), Some(y), Some(width), Some(height)) => Some(CanvasGeometry {
            x,
            y,
            width,
            height,
        }),
        _ => None,
    };
    let identity = value
        .get("customData")
        .and_then(|custom| custom.get("vistalith"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    Ok(ParsedElement {
        shape_id,
        shape_type,
        text,
        geometry,
        identity,
    })
}

/// One stored binding of a subject in a scene.
struct StoredBinding {
    binding: SubjectRef,
    subject: SubjectRef,
    scene: String,
    shape_id: String,
    fingerprint: String,
    via: String,
    geometry: Option<CanvasGeometry>,
}

fn stored_bindings(graph: &SemanticWorldGraph) -> Vec<StoredBinding> {
    graph
        .subjects()
        .filter(|node| {
            node.subject.namespace() == &Namespace::Vistalith
                && node.subject.kind() == &SubjectKind::SketchElement
        })
        .filter_map(|node| {
            let string_property = |key: &str| {
                node.properties
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            };
            let subject = string_property("subject")?;
            let subject = SubjectRef::parse(&subject).ok()?;
            Some(StoredBinding {
                binding: node.subject.clone(),
                subject,
                scene: string_property("scene").unwrap_or_else(|| DEFAULT_SCENE.to_owned()),
                shape_id: string_property("shape_id").unwrap_or_default(),
                fingerprint: string_property("fingerprint").unwrap_or_default(),
                via: string_property("via").unwrap_or_default(),
                geometry: node.properties.get("geometry").and_then(|v| {
                    serde_json::from_value::<CanvasGeometry>(v.clone()).ok()
                }),
            })
        })
        .collect()
}

/// Imports an Excalidraw scene (`{"elements": [...]}`) into the store as
/// durable `canvas-bound` events.
pub fn import_excalidraw(
    store: &mut GraphStore,
    scene: &serde_json::Value,
    options: &ExcalidrawImportOptions,
    actor: &Actor,
) -> Result<ExcalidrawImportReport, ExcalidrawError> {
    let elements = scene
        .get("elements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ExcalidrawError::Parse("scene has no `elements` array".to_owned()))?;
    let mut report = ExcalidrawImportReport::default();
    let mut provenance = Provenance::new(actor.as_str())
        .map_err(|e| ExcalidrawError::Store(e.to_string()))?;
    provenance.note = Some("excalidraw scene import".to_owned());

    for element in elements {
        let element = parse_element(element)?;
        let element_fingerprint = fingerprint(&element.shape_type, &element.text);

        // 1. Identity carried inside the scene file wins.
        if let Some(raw) = &element.identity {
            let subject = SubjectRef::parse(raw)
                .map_err(|_| ExcalidrawError::InvalidIdentity(raw.clone()))?;
            let bindings = stored_bindings(store.graph());
            if let Some(existing) = bindings.iter().find(|binding| {
                binding.scene == options.scene
                    && binding.subject == subject
                    && binding.fingerprint == element_fingerprint
            }) {
                report.skipped_bindings.push(existing.binding.clone());
                continue;
            }
            if store.graph().subject(&subject).is_none() {
                report.unknown_subjects.push(raw.clone());
                report.unbound_elements.push(element.shape_id.clone());
                continue;
            }
            append_binding(
                store,
                actor,
                subject.clone(),
                &options.scene,
                &element.shape_id,
                &element_fingerprint,
                VIA_CUSTOM_DATA,
                element.geometry,
            )?;
            report.bound.push(subject);
            continue;
        }

        // 2. No identity: resolve by content. A unique fingerprint match in
        //    this scene means the content is already bound — shape ids are
        //    irrelevant, so this is an idempotent skip, never a new binding.
        let matches: Vec<StoredBinding> = stored_bindings(store.graph())
            .into_iter()
            .filter(|binding| {
                binding.scene == options.scene && binding.fingerprint == element_fingerprint
            })
            .collect();
        let subjects: std::collections::BTreeSet<SubjectRef> =
            matches.iter().map(|binding| binding.subject.clone()).collect();
        if subjects.len() == 1 {
            let existing = matches.first().expect("checked len");
            report.skipped_bindings.push(existing.binding.clone());
            continue;
        }
        if subjects.len() > 1 {
            // Ambiguous: several subjects share this content — never guess.
            report.unbound_elements.push(element.shape_id.clone());
            continue;
        }

        // 3. Still unbound: optionally create a canvas note primitive
        //    (progressive formalization step 1, VISUAL-THINKING.md).
        if options.create_missing && !element.text.trim().is_empty() {
            let primitive = SubjectRef::new(
                Namespace::Vistalith,
                SubjectKind::Note,
                uuid::Uuid::now_v7().to_string(),
            )
            .expect("generated primitive id is valid");
            let event = import_event(
                actor,
                EventPayload::SubjectDefined(vistalith_domain::SubjectDefined {
                    subject: primitive.clone(),
                    authority: AuthorityClass::Advisory,
                    provenance: provenance.clone(),
                    properties: [
                        ("content".to_owned(), serde_json::json!(element.text)),
                        ("canvas_kind".to_owned(), serde_json::json!("note")),
                        ("from_scene".to_owned(), serde_json::json!(options.scene)),
                    ]
                    .into_iter()
                    .collect(),
                }),
                vec![primitive.clone()],
            );
            store.append(event).map_err(store_error)?;
            append_binding(
                store,
                actor,
                primitive.clone(),
                &options.scene,
                &element.shape_id,
                &element_fingerprint,
                VIA_IMPORT,
                element.geometry,
            )?;
            report.created_primitives.push(primitive);
            continue;
        }

        report.unbound_elements.push(element.shape_id.clone());
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn append_binding(
    store: &mut GraphStore,
    actor: &Actor,
    subject: SubjectRef,
    scene: &str,
    shape_id: &str,
    element_fingerprint: &str,
    via: &str,
    geometry: Option<CanvasGeometry>,
) -> Result<SubjectRef, ExcalidrawError> {
    let binding = SubjectRef::new(
        Namespace::Vistalith,
        SubjectKind::SketchElement,
        uuid::Uuid::now_v7().to_string(),
    )
    .expect("generated binding id is valid");
    let event = import_event(
        actor,
        EventPayload::CanvasBound(CanvasBound {
            binding: binding.clone(),
            subject: subject.clone(),
            scene: scene.to_owned(),
            shape_id: shape_id.to_owned(),
            fingerprint: element_fingerprint.to_owned(),
            via: via.to_owned(),
            geometry,
        }),
        vec![binding.clone(), subject],
    );
    store.append(event).map_err(store_error)?;
    Ok(binding)
}

/// Exports the canvas primitives as an Excalidraw scene. Every element
/// carries `customData.vistalith`; geometry comes from the latest stored
/// binding when available, else a deterministic grid.
pub fn export_excalidraw(graph: &SemanticWorldGraph, scene_name: &str) -> serde_json::Value {
    let bindings = stored_bindings(graph);
    let mut primitives: Vec<&crate::graph::SubjectNode> = graph
        .subjects()
        .filter(|node| {
            node.subject.namespace() == &Namespace::Vistalith
                && CANVAS_PRIMITIVE_KINDS.contains(node.subject.kind())
                && !node.deprecated
        })
        .collect();
    primitives.sort_by(|a, b| a.subject.to_string().cmp(&b.subject.to_string()));

    let elements: Vec<serde_json::Value> = primitives
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let text = node
                .properties
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let binding = bindings
                .iter()
                .filter(|binding| {
                    binding.subject == node.subject && binding.scene == scene_name
                })
                .max_by(|a, b| a.binding.id().cmp(b.binding.id()));
            let (x, y, width, height) = match binding.and_then(|binding| binding.geometry) {
                Some(geometry) => (geometry.x, geometry.y, geometry.width, geometry.height),
                None => {
                    let column = (index % 3) as f64;
                    let row = (index / 3) as f64;
                    (40.0 + column * 220.0, 40.0 + row * 90.0, 200.0, 60.0)
                }
            };
            serde_json::json!({
                "id": node.subject.id(),
                "type": "text",
                "x": x,
                "y": y,
                "width": width,
                "height": height,
                "text": text,
                "customData": { "vistalith": node.subject.to_string() },
            })
        })
        .collect();

    serde_json::json!({
        "type": "excalidraw",
        "version": 2,
        "source": "vistalith",
        "elements": elements,
        "appState": {
            "viewBackgroundColor": "#ffffff",
            "gridSize": null,
        },
    })
}

/// Read model for `GET /canvas/bindings`.
pub fn canvas_bindings(graph: &SemanticWorldGraph, scene: Option<&str>) -> Vec<CanvasBinding> {
    let mut bindings: Vec<CanvasBinding> = stored_bindings(graph)
        .into_iter()
        .filter(|binding| scene.is_none_or(|name| binding.scene == name))
        .map(|binding| CanvasBinding {
            binding: binding.binding,
            subject: binding.subject,
            scene: binding.scene,
            shape_id: binding.shape_id,
            fingerprint: binding.fingerprint,
            via: binding.via,
            geometry: binding.geometry,
        })
        .collect();
    bindings.sort_by(|a, b| a.binding.to_string().cmp(&b.binding.to_string()));
    bindings
}

fn store_error(err: StoreError) -> ExcalidrawError {
    ExcalidrawError::Store(err.to_string())
}

fn import_event(actor: &Actor, payload: EventPayload, subjects: Vec<SubjectRef>) -> VEvent {
    VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor.clone(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects,
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vistalith_domain::SubjectDefined;
    use std::collections::BTreeMap;

    fn actor() -> Actor {
        Actor::new("user:ruben").expect("static actor")
    }

    fn note(store: &mut GraphStore, id: &str, content: &str) -> SubjectRef {
        let subject = SubjectRef::new(Namespace::Vistalith, SubjectKind::Note, id.to_owned())
            .expect("valid id");
        let mut properties = BTreeMap::new();
        properties.insert("content".to_owned(), serde_json::json!(content));
        properties.insert("canvas_kind".to_owned(), serde_json::json!("note"));
        store
            .append(import_event(
                &actor(),
                EventPayload::SubjectDefined(SubjectDefined {
                    subject: subject.clone(),
                    authority: AuthorityClass::Advisory,
                    provenance: Provenance::new("user:ruben").unwrap(),
                    properties,
                }),
                vec![subject.clone()],
            ))
            .unwrap();
        subject
    }

    fn scene(elements: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "type": "excalidraw", "elements": elements })
    }

    fn element(id: &str, text: &str, identity: Option<&str>) -> serde_json::Value {
        let mut element = serde_json::json!({
            "id": id,
            "type": "text",
            "text": text,
            "x": 10.0,
            "y": 20.0,
            "width": 100.0,
            "height": 40.0,
        });
        if let Some(identity) = identity {
            element["customData"] = serde_json::json!({ "vistalith": identity });
        }
        element
    }

    #[test]
    fn fingerprint_is_whitespace_and_case_insensitive() {
        assert_eq!(fingerprint("text", "Hello  World"), fingerprint("text", "hello world"));
        assert_ne!(fingerprint("text", "hello"), fingerprint("text", "hello!"));
        // the shape type is part of the key
        assert_ne!(fingerprint("text", "hi"), fingerprint("rectangle", "hi"));
    }

    #[test]
    fn binds_by_custom_data_and_round_trips_as_noop() {
        let mut store = GraphStore::new();
        let subject = note(&mut store, "n-1", "Remember the milk");
        let identity = subject.to_string();

        let scene_one = scene(serde_json::json!([element("shape-1", "Remember the milk", Some(&identity))]));
        let report = import_excalidraw(
            &mut store,
            &scene_one,
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert_eq!(report.bound, vec![subject.clone()]);
        assert!(report.unbound_elements.is_empty());

        // same content, DIFFERENT shape id (renderer renumbered): still a no-op
        let scene_two = scene(serde_json::json!([element("shape-999", "Remember the milk", Some(&identity))]));
        let report = import_excalidraw(
            &mut store,
            &scene_two,
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert!(report.bound.is_empty());
        assert_eq!(report.skipped_bindings.len(), 1);
        assert_eq!(store.graph().revision(), 2);
    }

    #[test]
    fn content_stays_bound_when_custom_data_is_lost_and_ids_change() {
        let mut store = GraphStore::new();
        let subject = note(&mut store, "n-1", "Remember the milk");
        let identity = subject.to_string();
        import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("shape-1", "Remember the milk", Some(&identity))])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();

        // a foreign tool round-trip lost customData AND renumbered the id:
        // the same CONTENT still resolves to the same subject — no new
        // binding, nothing executed
        let before = store.graph().revision();
        let report = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("shape-7", "remember the MILK ", None)])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert!(report.bound.is_empty());
        assert_eq!(report.skipped_bindings.len(), 1);
        assert_eq!(store.graph().revision(), before);
        let bindings = canvas_bindings(store.graph(), Some(DEFAULT_SCENE));
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].subject, subject);
    }

    #[test]
    fn ambiguous_fingerprints_stay_unbound() {
        let mut store = GraphStore::new();
        note(&mut store, "n-1", "same text");
        note(&mut store, "n-2", "same text");
        let identity_one = SubjectRef::new(Namespace::Vistalith, SubjectKind::Note, "n-1")
            .unwrap()
            .to_string();
        let identity_two = SubjectRef::new(Namespace::Vistalith, SubjectKind::Note, "n-2")
            .unwrap()
            .to_string();
        import_excalidraw(
            &mut store,
            &scene(serde_json::json!([
                element("s-1", "same text", Some(&identity_one)),
                element("s-2", "same text", Some(&identity_two)),
            ])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();

        // a third shape with the same text but no customData: two possible
        // subjects — import refuses to guess
        let report = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("s-3", "same text", None)])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert_eq!(report.unbound_elements, vec!["s-3".to_owned()]);
    }

    #[test]
    fn create_missing_makes_note_primitives_for_unbound_text() {
        let mut store = GraphStore::new();
        let options = ExcalidrawImportOptions {
            scene: DEFAULT_SCENE.to_owned(),
            create_missing: true,
        };
        let report = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("s-1", "A fresh idea", None)])),
            &options,
            &actor(),
        )
        .unwrap();
        assert_eq!(report.created_primitives.len(), 1);
        let created_note = report.created_primitives[0].clone();
        let note_node = store.graph().subject(&created_note).unwrap();
        assert_eq!(note_node.subject.kind(), &SubjectKind::Note);
        assert_eq!(note_node.authority, AuthorityClass::Advisory);
        assert_eq!(
            note_node.properties["content"],
            serde_json::json!("A fresh idea")
        );
        // the advisory binding subject visualizes the new note
        let bindings = canvas_bindings(store.graph(), Some(DEFAULT_SCENE));
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].subject, created_note);
        assert_eq!(bindings[0].via, VIA_IMPORT);
        let binding_node = store.graph().subject(&bindings[0].binding).unwrap();
        assert_eq!(binding_node.subject.kind(), &SubjectKind::SketchElement);
        let visualized: Vec<_> = store
            .graph()
            .outgoing(&bindings[0].binding)
            .map(|edge| edge.relation.to.clone())
            .collect();
        assert_eq!(visualized, vec![created_note.clone()]);

        // and re-importing the same scene binds nothing new
        let report = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("s-1", "A fresh idea", None)])),
            &options,
            &actor(),
        )
        .unwrap();
        assert!(report.created_primitives.is_empty());
        assert_eq!(report.skipped_bindings.len(), 1);
    }

    #[test]
    fn unknown_custom_data_identity_is_reported_not_invented() {
        let mut store = GraphStore::new();
        let report = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element(
                "s-1",
                "ghost text",
                Some("arch:system:missing"),
            )])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert_eq!(report.unknown_subjects, vec!["arch:system:missing".to_owned()]);
        assert_eq!(report.unbound_elements, vec!["s-1".to_owned()]);
        assert_eq!(store.graph().revision(), 0);
    }

    #[test]
    fn broken_scenes_are_parse_errors() {
        let mut store = GraphStore::new();
        let error = import_excalidraw(
            &mut store,
            &serde_json::json!({ "type": "excalidraw" }),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap_err();
        assert!(matches!(error, ExcalidrawError::Parse(_)));
        let error = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([{ "type": "text", "text": "no id" }])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap_err();
        assert!(matches!(error, ExcalidrawError::Parse(_)));
        let error = import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("s-1", "x", Some("not-a-ref"))])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap_err();
        assert!(matches!(error, ExcalidrawError::InvalidIdentity(_)));
    }

    #[test]
    fn export_embeds_identity_and_reuses_binding_geometry() {
        let mut store = GraphStore::new();
        let subject = note(&mut store, "n-1", "Remember the milk");
        let identity = subject.to_string();
        import_excalidraw(
            &mut store,
            &scene(serde_json::json!([element("shape-1", "Remember the milk", Some(&identity))])),
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();

        let exported = export_excalidraw(store.graph(), DEFAULT_SCENE);
        let elements = exported["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["customData"]["vistalith"], serde_json::json!(identity));
        // geometry came from the stored binding, not the fallback grid
        assert_eq!(elements[0]["x"], serde_json::json!(10.0));
        assert_eq!(elements[0]["y"], serde_json::json!(20.0));

        // the export re-imports as a no-op (same fingerprint + identity)
        let before = store.graph().revision();
        let report = import_excalidraw(
            &mut store,
            &exported,
            &ExcalidrawImportOptions::default(),
            &actor(),
        )
        .unwrap();
        assert!(report.bound.is_empty());
        assert_eq!(report.skipped_bindings.len(), 1);
        assert_eq!(store.graph().revision(), before);
    }

    #[test]
    fn export_uses_deterministic_grid_without_bindings() {
        let mut store = GraphStore::new();
        note(&mut store, "b", "beta");
        note(&mut store, "a", "alpha");
        let exported = export_excalidraw(store.graph(), DEFAULT_SCENE);
        let elements = exported["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        // canonically ordered: a before b, grid positions differ
        assert_eq!(elements[0]["text"], serde_json::json!("alpha"));
        assert_ne!(elements[0]["x"], elements[1]["x"]);
    }

    #[test]
    fn bindings_read_model_filters_by_scene() {
        let mut store = GraphStore::new();
        let subject = note(&mut store, "n-1", "shared content");
        let identity = subject.to_string();
        for (scene_name, shape) in [
            ("design", "shape-a"),
            ("review", "shape-b"),
        ] {
            import_excalidraw(
                &mut store,
                &serde_json::json!({ "elements": [{
                    "id": shape,
                    "type": "text",
                    "text": "shared content",
                    "customData": { "vistalith": identity },
                }]}),
                &ExcalidrawImportOptions {
                    scene: scene_name.to_owned(),
                    create_missing: false,
                },
                &actor(),
            )
            .unwrap();
        }
        assert_eq!(canvas_bindings(store.graph(), None).len(), 2);
        assert_eq!(canvas_bindings(store.graph(), Some("design")).len(), 1);
        assert_eq!(canvas_bindings(store.graph(), Some("nope")).len(), 0);
    }
}

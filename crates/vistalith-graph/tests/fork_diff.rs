//! Fork / diff / time travel invariants (SPEC-011, slice 5): the graph at an
//! earlier revision is exactly a strict replay of the log prefix, structural
//! diffs are deterministic, and forked threads link back with preserved
//! item bindings.

use vistalith_domain::{
    Actor, EventPayload, MessageAppended, MessageRole, Namespace, SubjectKind, SubjectRef,
    ThreadForked, ThreadStarted, VEvent,
};
use vistalith_graph::{GraphStore, diff_graphs};

fn actor() -> Actor {
    Actor::new("system:test").expect("static actor")
}

fn thread(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Agentic, SubjectKind::Thread, id).unwrap()
}

fn message(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Agentic, SubjectKind::Message, id).unwrap()
}

fn event(payload: EventPayload, subjects: Vec<SubjectRef>) -> VEvent {
    VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects,
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}

/// thread "t1" with two turns; returns the store and its final revision.
fn conversation_store() -> (GraphStore, u64) {
    let mut store = GraphStore::new();
    store
        .append(event(
            EventPayload::ThreadStarted(ThreadStarted {
                thread: thread("t1"),
                title: "explore".to_owned(),
            }),
            vec![thread("t1")],
        ))
        .unwrap();
    for turn in 1..=2u64 {
        store
            .append(event(
                EventPayload::MessageAppended(MessageAppended {
                    thread: thread("t1"),
                    message: message(&format!("m{}", turn * 2 - 1)),
                    role: MessageRole::User,
                    content: format!("question {turn}"),
                    turn,
                    forked_of: None,
                    mentions: Vec::new(),
                }),
                vec![thread("t1"), message(&format!("m{}", turn * 2 - 1))],
            ))
            .unwrap();
        store
            .append(event(
                EventPayload::MessageAppended(MessageAppended {
                    thread: thread("t1"),
                    message: message(&format!("m{}", turn * 2)),
                    role: MessageRole::Assistant,
                    content: format!("answer {turn}"),
                    turn,
                    forked_of: None,
                    mentions: Vec::new(),
                }),
                vec![thread("t1"), message(&format!("m{}", turn * 2))],
            ))
            .unwrap();
    }
    let revision = store.graph().revision();
    (store, revision)
}

#[test]
fn graph_at_revision_is_exact_prefix_replay() {
    let (store, current) = conversation_store();
    let at_one = store.graph_at_revision(3).expect("revision 3 exists");
    assert_eq!(at_one.revision(), 3);
    // Turn 2 did not happen yet at revision 3 (thread + msg + msg = 3 events).
    let m3 = message("m3");
    assert!(at_one.subject(&m3).is_none());
    assert!(store.graph().subject(&m3).is_some());

    // Determinism: cutting at the current revision reproduces the live graph.
    let full = store.graph_at_revision(current).unwrap();
    assert_eq!(
        vistalith_graph::graph_digest(&full),
        store.digest(),
        "time travel to HEAD must equal the live projection"
    );

    let unknown = store.graph_at_revision(current + 1);
    assert!(matches!(unknown, Err(vistalith_graph::StoreError::UnknownRevision(..))));
}

#[test]
fn diff_between_revisions_reports_exactly_what_changed() {
    let (store, current) = conversation_store();

    // Revisions 0 -> current: everything is "added".
    let everything = store.diff_revisions(0, current).unwrap();
    assert_eq!(everything.added_subjects.len(), 5);
    assert!(everything.removed_subjects.is_empty());

    // Revisions 3 -> 4: exactly one event separates them (message m3);
    // nothing else changed.
    let step = store.diff_revisions(3, 4).unwrap();
    assert_eq!(step.added_subjects, vec![message("m3")]);
    assert!(step.removed_subjects.is_empty());
    assert!(step.changed_subjects.is_empty());
    assert_eq!(step.added_relations.len(), 1);
    assert!(step.changed_relations.is_empty());
    assert!(!step.is_empty());

    // Determinism: the same pair of revisions always yields the same diff.
    let again = store.diff_revisions(3, 4).unwrap();
    assert_eq!(step, again);
}

#[test]
fn diff_detects_property_and_deprecation_changes() {
    use vistalith_domain::{AuthorityClass, Provenance, SubjectDefined, SubjectDeprecated, SubjectUpdated};
    use vistalith_graph::apply_event;

    let provenance = Provenance::new("system:test").unwrap();
    let build = |deprecated: bool, extra: bool| {
        let subject = SubjectRef::new(Namespace::Arch, SubjectKind::Container, "svc").unwrap();
        let mut graph = vistalith_graph::SemanticWorldGraph::new();
        let mut props = std::collections::BTreeMap::new();
        props.insert("status".to_owned(), serde_json::json!("running"));
        apply_event(
            &mut graph,
            &event(
                EventPayload::SubjectDefined(SubjectDefined {
                    subject: subject.clone(),
                    authority: AuthorityClass::Authoritative,
                    provenance: provenance.clone(),
                    properties: props.clone(),
                }),
                vec![subject.clone()],
            ),
            0,
        )
        .unwrap();
        if extra {
            let mut updated = std::collections::BTreeMap::new();
            updated.insert("status".to_owned(), serde_json::json!("deprecated"));
            updated.insert("owner".to_owned(), serde_json::json!("team-b"));
            apply_event(
                &mut graph,
                &event(
                    EventPayload::SubjectUpdated(SubjectUpdated {
                        subject: subject.clone(),
                        properties: updated,
                    }),
                    vec![subject.clone()],
                ),
                1,
            )
            .unwrap();
        }
        if deprecated {
            apply_event(
                &mut graph,
                &event(
                    EventPayload::SubjectDeprecated(SubjectDeprecated {
                        subject: subject.clone(),
                        reason: None,
                    }),
                    vec![subject.clone()],
                ),
                2,
            )
            .unwrap();
        }
        graph
    };

    let graph_a = build(false, false);
    let graph_b = build(true, true);
    let diff = diff_graphs(&graph_a, &graph_b);
    assert!(diff.added_subjects.is_empty());
    assert_eq!(diff.changed_subjects.len(), 1);
    let change = &diff.changed_subjects[0];
    let keys: Vec<_> = change.changes.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["deprecated", "owner", "status"]);
}

#[test]
fn forked_thread_projection_preserves_bindings_and_links_back() {
    let (mut store, _) = conversation_store();
    let fork = thread("f1");
    store
        .append(event(
            EventPayload::ThreadForked(ThreadForked {
                fork: fork.clone(),
                source: thread("t1"),
                up_to_turn: 1,
                note: Some("what if the model were cheaper?".to_owned()),
            }),
            vec![fork.clone(), thread("t1")],
        ))
        .unwrap();
    // Copied item: a new message in the fork bound to its original.
    store
        .append(event(
            EventPayload::MessageAppended(MessageAppended {
                thread: fork.clone(),
                message: message("fm1"),
                role: MessageRole::User,
                content: "question 1".to_owned(),
                turn: 1,
                forked_of: Some(message("m1")),
                mentions: Vec::new(),
            }),
            vec![fork.clone(), message("m1")],
        ))
        .unwrap();

    let node = store.graph().subject(&fork).expect("fork thread exists");
    assert_eq!(node.properties["turns"], serde_json::json!(1));
    assert_eq!(
        node.properties["forked_from"],
        serde_json::json!("agentic:thread:t1")
    );
    let copied = store.graph().subject(&message("fm1")).unwrap();
    assert_eq!(
        copied.properties["forked_of"],
        serde_json::json!("agentic:message:m1"),
        "SPEC-011: the copied item keeps its semantic binding"
    );
    // The fork links back to the source with a typed relation.
    let link = store
        .graph()
        .relations_of_kind(&vistalith_domain::RelationKind::ForkedFrom)
        .find(|f| f.relation.from == fork)
        .expect("forked_from relation exists");
    assert_eq!(link.relation.to, thread("t1"));

    // Replaying the same log yields the same state (determinism).
    let json = store.to_log_json();
    let rebuilt = GraphStore::from_stored_json(&json).unwrap();
    assert_eq!(rebuilt.digest(), store.digest());
}

//! Gate checks that run on every `cargo test` (in-memory engine, small
//! scale). The scale/durability/footprint numbers come from the
//! `surrealdb-spike` binary run; these tests keep the *invariants* green:
//! deterministic rebuild, domain-boundary digest equality, idempotent
//! migrations, namespace isolation, deterministic edge keys.

use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;
use vistalith_graph::GraphStore;
use vistalith_spike_surrealdb as spike;

async fn fresh_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("mem engine opens");
    db.use_ns("spike-test").use_db("spike-test").await.expect("ns/db set");
    spike::define_schema(&db).await.expect("schema applies");
    db
}

#[tokio::test]
async fn schema_application_is_idempotent() {
    let db = fresh_db().await;
    // Second and third application must succeed unchanged (OVERWRITE).
    spike::define_schema(&db).await.expect("second application");
    spike::define_schema(&db).await.expect("third application");
}

#[tokio::test]
async fn replay_is_deterministic_and_matches_the_in_memory_projection() {
    let events = spike::synthetic_log(60);

    let db_a = fresh_db().await;
    spike::replay_log(&db_a, &events).await.expect("replay a");
    let (subjects_a, relations_a) = spike::facts_from_surreal(&db_a).await.unwrap();
    let digest_a = spike::facts_digest(&subjects_a, &relations_a);

    let db_b = fresh_db().await;
    spike::replay_log(&db_b, &events).await.expect("replay b");
    let (subjects_b, relations_b) = spike::facts_from_surreal(&db_b).await.unwrap();
    let digest_b = spike::facts_digest(&subjects_b, &relations_b);

    assert_eq!(digest_a, digest_b, "two fresh replays must agree");

    // No lock-in at the domain boundary: the same log projected by the SWG
    // (in-memory, strict semantics) produces the same canonical facts.
    let store = GraphStore::replay(events.iter().cloned()).expect("in-memory replay");
    let (mem_subjects, mem_relations) = spike::facts_from_graph(store.graph());
    let digest_mem = spike::facts_digest(&mem_subjects, &mem_relations);
    assert_eq!(digest_a, digest_mem, "surreal rows == in-memory projection");
    assert_eq!(subjects_a.len(), 60);
}

#[tokio::test]
async fn replaying_the_same_events_twice_is_idempotent() {
    let events = spike::synthetic_log(24);
    let db = fresh_db().await;
    spike::replay_log(&db, &events).await.expect("first replay");
    let (subjects, relations) = spike::facts_from_surreal(&db).await.unwrap();
    let first = spike::facts_digest(&subjects, &relations);

    spike::replay_log(&db, &events).await.expect("second replay");
    let (subjects, relations) = spike::facts_from_surreal(&db).await.unwrap();
    let second = spike::facts_digest(&subjects, &relations);
    assert_eq!(first, second, "duplicate replay must not duplicate facts");
}

#[tokio::test]
async fn native_traversal_returns_deduplicated_targets() {
    let events = spike::synthetic_log(12);
    let db = fresh_db().await;
    spike::replay_log(&db, &events).await.expect("replay");

    // The fixed pattern for 12 nodes: ring i -> i+1 plus chords i -> (i*7+13)
    // % 12 on multiples of 3 (3 -> 10, 6 -> 7, 9 -> 4; 0's chord target
    // coincides with its ring edge and is dropped). Native traversal returns
    // the deduplicated target set per hop chain.
    let seed0 = spike::synthetic_seed();
    assert_eq!(spike::traverse(&db, &seed0, 1).await.unwrap(), 1);
    assert_eq!(spike::traverse(&db, &seed0, 3).await.unwrap(), 1);

    let seed3 = spike::synthetic_subject(3).to_string();
    assert_eq!(spike::traverse(&db, &seed3, 1).await.unwrap(), 2);
    assert_eq!(spike::traverse(&db, &seed3, 3).await.unwrap(), 2);
}

#[tokio::test]
async fn namespaces_are_isolated() {
    let events = spike::synthetic_log(6);
    let db = fresh_db().await;
    spike::replay_log(&db, &events).await.expect("replay");
    assert!(spike::count_rows(&db, "subject").await.unwrap() > 0);

    db.use_ns("other-namespace").use_db("spike-test").await.unwrap();
    assert_eq!(
        spike::count_rows(&db, "subject").await.unwrap(),
        0,
        "project-per-database isolation must be trivial"
    );
}

#[tokio::test]
async fn non_storage_events_are_counted_not_stored() {
    use vistalith_domain::{EventPayload, Namespace, SubjectKind, SubjectRef, ThreadStarted};
    let db = fresh_db().await;
    let thread = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::Thread,
        "t-1".to_owned(),
    )
    .unwrap();
    let actor = vistalith_domain::Actor::new("system:test").unwrap();
    let event = vistalith_domain::VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor,
        timestamp: time::OffsetDateTime::now_utc(),
        subjects: vec![thread.clone()],
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload: EventPayload::ThreadStarted(ThreadStarted {
            thread,
            title: "spike probe".to_owned(),
        }),
    };
    let stored = spike::replay_log(&db, &[event]).await.unwrap();
    assert_eq!(stored.skipped, 1);
    assert_eq!(spike::count_rows(&db, "subject").await.unwrap(), 0);
}

#[cfg(feature = "file-engine")]
#[tokio::test]
async fn file_engine_durably_reopens() {
    let dir = std::env::temp_dir().join(format!(
        "vistalith-spike-durability-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let events = spike::synthetic_log(30);
    let digest;
    {
        let db = Surreal::new::<surrealdb::engine::local::SurrealKv>(&*dir)
            .await
            .expect("surrealkv opens");
        db.use_ns("spike-test").use_db("spike-test").await.unwrap();
        spike::define_schema(&db).await.unwrap();
        spike::replay_log(&db, &events).await.unwrap();
        let (subjects, relations) = spike::facts_from_surreal(&db).await.unwrap();
        digest = spike::facts_digest(&subjects, &relations);
        // Dropping closes the file engine.
    }

    let reopened = spike::open_surrealkv_with_retry(&dir, 8)
        .await
        .expect("surrealkv reopens after close");
    reopened.use_ns("spike-test").use_db("spike-test").await.unwrap();
    let (subjects, relations) = spike::facts_from_surreal(&reopened).await.unwrap();
    assert_eq!(spike::facts_digest(&subjects, &relations), digest);
    let _ = std::fs::remove_dir_all(&dir);
}

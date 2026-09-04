//! SurrealDB embedded storage spike (SPK-003, slice 5).
//!
//! Proves — or refutes — the decision gate in the baseline's
//! `technology/GRAPH-STORAGE-DECISION.md` against the stable 3.2.x line:
//! deterministic migrations/rebuild, binary/startup footprint, local
//! durability, traversal latency, and no lock-in at the domain boundary.
//!
//! Design rules:
//! - The domain vocabulary (`vistalith-domain`) is the only input. Nothing
//!   SurrealQL-shaped leaks into the domain: this crate converts at the
//!   boundary and could be deleted without touching it.
//! - The projection mirrors the SWG's strict projection for the two storage
//!   facts (subjects and relations): `subject-defined` upserts a `subject`
//!   record keyed by the canonical `ns:kind:id` identity; `relation-declared`
//!   creates a native graph edge with a deterministic record key, so replay
//!   is idempotent and byte-stable.
//! - Determinism is measured, not assumed: the canonical fact form built
//!   from SurrealDB rows must equal the one built from the in-memory
//!   `SemanticWorldGraph`, and two independent replays must agree.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use surrealdb::types::SurrealValue;
use surrealdb::{Connection, Surreal};
use vistalith_domain::{
    Actor, EventPayload, Namespace, RelationDeclared, RelationFact, RelationKind, SubjectDefined,
    SubjectKind, SubjectRef, VEvent,
};
use vistalith_graph::SemanticWorldGraph;

/// Schema definition, applied with `OVERWRITE` so running it any number of
/// times on any state produces the same schema — the reproducible-migration
/// question of the spike.
pub const SCHEMA_SQL: &str = "
DEFINE TABLE OVERWRITE subject SCHEMALESS;
DEFINE INDEX OVERWRITE subject_by_namespace ON TABLE subject COLUMNS namespace;
DEFINE TABLE OVERWRITE relates TYPE RELATION IN subject OUT subject SCHEMALESS;
";

#[derive(Debug, Error)]
pub enum SpikeError {
    #[error("surrealdb error: {0}")]
    Db(#[from] surrealdb::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// One projected storage fact: a subject row, keyed by its canonical
/// `ns:kind:id` identity (which is also the Surreal record key).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactSubject {
    pub identity: String,
    pub namespace: String,
    pub kind: String,
    pub authority: String,
    pub deprecated: bool,
    pub properties: Value,
}

/// One projected storage fact: a native relation edge with a deterministic
/// key derived from `(from, kind, to)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, Eq, Ord)]
pub struct FactRelation {
    pub from: String,
    pub kind: String,
    pub to: String,
    pub authority: String,
}

/// Canonical, byte-stable JSON of the fact set — the same shape is built
/// from SurrealDB rows and from the in-memory graph, so digest equality is
/// the "no lock-in at the domain boundary" check.
#[derive(Serialize)]
struct CanonicalFacts {
    subjects: Vec<FactSubject>,
    relations: Vec<FactRelation>,
}

pub fn canonical_facts_json(subjects: &[FactSubject], relations: &[FactRelation]) -> String {
    let mut subjects = subjects.to_vec();
    subjects.sort_by(|a, b| a.identity.cmp(&b.identity));
    let mut relations = relations.to_vec();
    relations.sort();
    let canonical = CanonicalFacts { subjects, relations };
    serde_json::to_string(&canonical).expect("canonical fact serialization cannot fail")
}

pub fn facts_digest(subjects: &[FactSubject], relations: &[FactRelation]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_facts_json(subjects, relations).as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extracts the same fact form from the in-memory projection.
pub fn facts_from_graph(graph: &SemanticWorldGraph) -> (Vec<FactSubject>, Vec<FactRelation>) {
    let subjects = graph
        .subjects()
        .map(|node| FactSubject {
            identity: node.subject.to_string(),
            namespace: node.subject.namespace().to_string(),
            kind: node.subject.kind().to_string(),
            authority: authority_wire(node.authority),
            deprecated: node.deprecated,
            properties: Value::Object(node.properties.clone().into_iter().collect()),
        })
        .collect();
    let relations = graph
        .relations()
        .map(|fact| FactRelation {
            from: fact.relation.from.to_string(),
            kind: fact.relation.kind.as_str().to_owned(),
            to: fact.relation.to.to_string(),
            authority: authority_wire(fact.authority),
        })
        .collect();
    (subjects, relations)
}

fn authority_wire(authority: vistalith_domain::AuthorityClass) -> String {
    serde_json::to_string(&authority)
        .expect("authority serialization cannot fail")
        .trim_matches('"')
        .to_owned()
}

// --- record id helpers ------------------------------------------------------

/// Escapes a string for a SurrealQL single-quoted literal.
fn surreal_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Deterministic record id: `subject:['<ns:kind:id>']`. The canonical
/// identity *is* the storage key — no surrogate ids to migrate.
fn subject_record(identity: &str) -> String {
    format!("subject:[{}]", surreal_string_literal(identity))
}

/// Deterministic edge key `relates:['from|kind|to']` so replays are
/// idempotent (RELATE overwrites an existing record id).
pub fn edge_key(from: &str, kind: &str, to: &str) -> String {
    format!("{from}|{kind}|{to}")
}

fn edge_record(from: &str, kind: &str, to: &str) -> String {
    format!(
        "relates:[{}]",
        surreal_string_literal(&edge_key(from, kind, to))
    )
}

// --- projection: events -> SurrealDB ----------------------------------------

/// Applies the schema. Idempotent by construction (`OVERWRITE` defines),
/// which is the reproducible-migration property under test.
pub async fn define_schema<C: Connection>(db: &Surreal<C>) -> Result<(), SpikeError> {
    db.query(SCHEMA_SQL).await?.check()?;
    Ok(())
}

/// Outcome of projecting one event into the storage layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredAs {
    Subject,
    Relation,
    /// Not a storage fact (conversation/intent events own no SWG storage
    /// facts beyond what other events already project). Counted, not stored.
    Skipped,
}

/// Projects one event into SurrealDB with bound parameters. This is the
/// correctness path: arbitrary identities and properties, one round trip per
/// event.
pub async fn replay_event<C: Connection>(
    db: &Surreal<C>,
    event: &VEvent,
) -> Result<StoredAs, SpikeError> {
    match &event.payload {
        EventPayload::SubjectDefined(defined) => {
            upsert_subject_bound(db, defined).await?;
            Ok(StoredAs::Subject)
        }
        EventPayload::RelationDeclared(declared) => {
            relate_bound(db, declared).await?;
            Ok(StoredAs::Relation)
        }
        _ => Ok(StoredAs::Skipped),
    }
}

async fn upsert_subject_bound<C: Connection>(
    db: &Surreal<C>,
    defined: &SubjectDefined,
) -> Result<(), SpikeError> {
    let sql = format!(
        "UPSERT {} SET identity = $identity, namespace = $namespace, kind = $kind, \
         authority = $authority, deprecated = false, properties = $properties;",
        subject_record(&defined.subject.to_string())
    );
    db.query(sql)
        .bind(("identity", defined.subject.to_string()))
        .bind(("namespace", defined.subject.namespace().to_string()))
        .bind(("kind", defined.subject.kind().to_string()))
        .bind(("authority", authority_wire(defined.authority)))
        .bind(("properties", defined.properties.clone()))
        .await?
        .check()?;
    Ok(())
}

async fn relate_bound<C: Connection>(
    db: &Surreal<C>,
    declared: &RelationDeclared,
) -> Result<(), SpikeError> {
    let fact: &RelationFact = &declared.fact;
    let sql = format!(
        "RELATE {}->{}->{} SET from_identity = $from, to_identity = $to, kind = $kind, \
         authority = $authority;",
        subject_record(&fact.relation.from.to_string()),
        edge_record(
            &fact.relation.from.to_string(),
            fact.relation.kind.as_str(),
            &fact.relation.to.to_string()
        ),
        subject_record(&fact.relation.to.to_string()),
    );
    db.query(sql)
        .bind(("from", fact.relation.from.to_string()))
        .bind(("to", fact.relation.to.to_string()))
        .bind(("kind", fact.relation.kind.as_str()))
        .bind(("authority", authority_wire(fact.authority)))
        .await?
        .check()?;
    Ok(())
}

/// Replays a whole log (the correctness path above, event by event).
pub async fn replay_log<C: Connection>(
    db: &Surreal<C>,
    events: &[VEvent],
) -> Result<RebuildStats, SpikeError> {
    let started = Instant::now();
    let mut subjects = 0usize;
    let mut relations = 0usize;
    let mut skipped = 0usize;
    for event in events {
        match replay_event(db, event).await? {
            StoredAs::Subject => subjects += 1,
            StoredAs::Relation => relations += 1,
            StoredAs::Skipped => skipped += 1,
        }
    }
    Ok(RebuildStats {
        subjects,
        relations,
        skipped,
        statements: subjects + relations,
        queries: subjects + relations,
        elapsed: started.elapsed(),
    })
}

// --- bulk load: scale path ---------------------------------------------------

/// One inline SQL statement per fact (no bindings): the fast path used for
/// the 100k/1m-edge rebuild measurement. Identities and properties come from
/// the synthetic generator, and every string is still escaped.
pub fn subject_upsert_sql(subject: &SubjectRef, properties: &Value) -> String {
    format!(
        "UPSERT {} SET identity = {}, namespace = {}, kind = {}, authority = {}, \
         deprecated = false, properties = {};",
        subject_record(&subject.to_string()),
        surreal_string_literal(&subject.to_string()),
        surreal_string_literal(&subject.namespace().to_string()),
        surreal_string_literal(&subject.kind().to_string()),
        surreal_string_literal("authoritative"),
        properties
    )
}

pub fn relation_declare_sql(from: &SubjectRef, kind: RelationKind, to: &SubjectRef) -> String {
    format!(
        "RELATE {}->{}->{} SET from_identity = {}, to_identity = {}, kind = {}, authority = {};",
        subject_record(&from.to_string()),
        edge_record(&from.to_string(), kind.as_str(), &to.to_string()),
        subject_record(&to.to_string()),
        surreal_string_literal(&from.to_string()),
        surreal_string_literal(&to.to_string()),
        surreal_string_literal(kind.as_str()),
        surreal_string_literal("authoritative"),
    )
}

/// Applies statements in fixed-size batches (multi-statement queries), the
/// way a real storage adapter would amortize round trips.
pub async fn bulk_apply<C: Connection>(
    db: &Surreal<C>,
    statements: &[String],
    batch: usize,
) -> Result<RebuildStats, SpikeError> {
    let started = Instant::now();
    let mut queries = 0usize;
    for chunk in statements.chunks(batch.max(1)) {
        let sql = chunk.join("\n");
        db.query(sql).await?.check()?;
        queries += 1;
    }
    Ok(RebuildStats {
        subjects: 0,
        relations: 0,
        skipped: 0,
        statements: statements.len(),
        queries,
        elapsed: started.elapsed(),
    })
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RebuildStats {
    pub subjects: usize,
    pub relations: usize,
    pub skipped: usize,
    pub statements: usize,
    pub queries: usize,
    #[serde(serialize_with = "serialize_millis")]
    pub elapsed: Duration,
}

fn serialize_millis<S: serde::Serializer>(
    value: &Duration,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(value.as_secs_f64() * 1000.0)
}

// --- reading back -------------------------------------------------------------

/// Opens a SurrealKV database, retrying briefly: closing an embedded engine
/// is asynchronous (dropping the client signals a background shutdown), so an
/// immediate reopen can race the release of the storage LOCK.
#[cfg(feature = "file-engine")]
pub async fn open_surrealkv_with_retry(
    path: &std::path::Path,
    attempts: usize,
) -> Result<Surreal<surrealdb::engine::local::Db>, SpikeError> {
    let mut delay = Duration::from_millis(200);
    let mut last = None;
    for _ in 0..attempts.max(1) {
        match Surreal::new::<surrealdb::engine::local::SurrealKv>(&*path.to_path_buf()).await {
            Ok(db) => return Ok(db),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(2));
            }
        }
    }
    Err(SpikeError::Db(last.expect("at least one attempt")))
}

#[derive(Debug, SurrealValue)]
struct SubjectRow {
    identity: String,
    namespace: String,
    kind: String,
    authority: String,
    deprecated: bool,
    properties: Value,
}

#[derive(Debug, SurrealValue)]
struct RelationRow {
    from_identity: String,
    to_identity: String,
    kind: String,
    authority: String,
}

/// Reads all facts back from SurrealDB in canonical order.
pub async fn facts_from_surreal<C: Connection>(
    db: &Surreal<C>,
) -> Result<(Vec<FactSubject>, Vec<FactRelation>), SpikeError> {
    let mut subject_response = db
        .query("SELECT identity, namespace, kind, authority, deprecated, properties FROM subject;")
        .await?;
    let subject_rows: Vec<SubjectRow> = subject_response.take(0)?;
    let mut relation_response = db
        .query(
            "SELECT from_identity, to_identity, kind, authority FROM relates \
             ORDER BY from_identity, kind, to_identity;",
        )
        .await?;
    let relation_rows: Vec<RelationRow> = relation_response.take(0)?;
    let subjects = subject_rows
        .into_iter()
        .map(|row| FactSubject {
            identity: row.identity,
            namespace: row.namespace,
            kind: row.kind,
            authority: row.authority,
            deprecated: row.deprecated,
            properties: row.properties,
        })
        .collect();
    let relations = relation_rows
        .into_iter()
        .map(|row| FactRelation {
            from: row.from_identity,
            kind: row.kind,
            to: row.to_identity,
            authority: row.authority,
        })
        .collect();
    Ok((subjects, relations))
}

pub async fn count_rows<C: Connection>(db: &Surreal<C>, table: &str) -> Result<usize, SpikeError> {
    let sql = format!("SELECT VALUE count() FROM {table} GROUP ALL;");
    let mut response = db.query(&sql).await?;
    // Aggregates come back as `{ count: n }` objects in 3.x; a namespace
    // without that table surfaces at take() as NotFound — empty, not broken.
    let rows: Vec<Value> = match response.take(0) {
        Ok(rows) => rows,
        Err(e) if e.to_string().contains("does not exist") => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    Ok(rows
        .first()
        .and_then(|row| row.get("count").and_then(|c| c.as_u64()))
        .unwrap_or(0) as usize)
}

// --- traversal ------------------------------------------------------------------

/// One N-hop native graph traversal from `seed`. Returns the number of
/// records reached: multi-hop chains come back nested per hop, so nested
/// arrays are counted recursively (this is a latency probe, not a set op).
pub async fn traverse<C: Connection>(
    db: &Surreal<C>,
    seed: &str,
    hops: usize,
) -> Result<usize, SpikeError> {
    let mut chain = String::new();
    for _ in 0..hops.max(1) {
        chain.push_str("->relates->subject");
    }
    let sql = format!("SELECT VALUE {chain} FROM {};", subject_record(seed));
    let mut response = db.query(&sql).await?;
    let rows: Vec<Value> = response.take(0)?;
    Ok(rows.iter().map(count_leaves).sum())
}

fn count_leaves(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.iter().map(count_leaves).sum(),
        _ => 1,
    }
}

/// Measures `runs` traversals and summarizes latency.
pub async fn measure_traversal<C: Connection>(
    db: &Surreal<C>,
    seed: &str,
    hops: usize,
    runs: usize,
) -> Result<LatencyStats, SpikeError> {
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        traverse(db, seed, hops).await?;
        samples.push(started.elapsed());
    }
    Ok(summarize(samples))
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LatencyStats {
    pub runs: usize,
    #[serde(serialize_with = "serialize_micros")]
    pub p50: Duration,
    #[serde(serialize_with = "serialize_micros")]
    pub p95: Duration,
    #[serde(serialize_with = "serialize_micros")]
    pub min: Duration,
    #[serde(serialize_with = "serialize_micros")]
    pub max: Duration,
}

fn serialize_micros<S: serde::Serializer>(
    value: &Duration,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(value.as_secs_f64() * 1_000_000.0)
}

pub fn summarize(mut samples: Vec<Duration>) -> LatencyStats {
    samples.sort();
    let pick = |p: f64| -> Duration {
        if samples.is_empty() {
            return Duration::ZERO;
        }
        let index = ((p / 100.0) * (samples.len() - 1) as f64).round() as usize;
        samples[index.min(samples.len() - 1)]
    };
    LatencyStats {
        runs: samples.len(),
        p50: pick(50.0),
        p95: pick(95.0),
        min: samples.first().copied().unwrap_or(Duration::ZERO),
        max: samples.last().copied().unwrap_or(Duration::ZERO),
    }
}

// --- synthetic workload -------------------------------------------------------------

fn spike_actor() -> Actor {
    Actor::new("system:vistalith-spike").expect("static actor")
}

/// Deterministic synthetic log: `nodes` subjects on a ring (i -> i+1) plus
/// chords every third node (i -> (i*7+13) % nodes) so multi-hop traversals
/// return real data. Ring size and chord pattern are fixed, so the same
/// call always yields the same event sequence (modulo uuid/timestamps,
/// which never enter canonical facts).
pub fn synthetic_log(nodes: usize) -> Vec<VEvent> {
    let nodes = nodes.max(2);
    let actor = spike_actor();
    let timestamp = time::OffsetDateTime::now_utc();
    let mut events = Vec::with_capacity(nodes * 2);
    for i in 0..nodes {
        let subject = synthetic_subject(i);
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("index".to_owned(), serde_json::json!(i));
        events.push(VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: actor.clone(),
            timestamp,
            subjects: vec![subject.clone()],
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: EventPayload::SubjectDefined(SubjectDefined {
                subject,
                authority: vistalith_domain::AuthorityClass::Authoritative,
                provenance: vistalith_domain::Provenance {
                    source: spike_actor(),
                    source_revision: None,
                    note: Some("spike synthetic workload".to_owned()),
                    confidence: None,
                },
                properties,
            }),
        });
    }
    for i in 0..nodes {
        let successor = synthetic_subject((i + 1) % nodes);
        events.push(relation_event(&actor, timestamp, synthetic_subject(i), successor));
        if i % 3 == 0 {
            let chord_target = (i * 7 + 13) % nodes;
            if chord_target != i && chord_target != (i + 1) % nodes {
                events.push(relation_event(
                    &actor,
                    timestamp,
                    synthetic_subject(i),
                    synthetic_subject(chord_target),
                ));
            }
        }
    }
    events
}

fn relation_event(
    actor: &Actor,
    timestamp: time::OffsetDateTime,
    from: SubjectRef,
    to: SubjectRef,
) -> VEvent {
    VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor.clone(),
        timestamp,
        subjects: vec![from.clone(), to.clone()],
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload: EventPayload::RelationDeclared(RelationDeclared {
            fact: RelationFact {
                relation: vistalith_domain::RelationRef::new(from, RelationKind::DependsOn, to)
                    .expect("distinct synthetic endpoints"),
                authority: vistalith_domain::AuthorityClass::Authoritative,
                provenance: vistalith_domain::Provenance {
                    source: spike_actor(),
                    source_revision: None,
                    note: Some("spike synthetic workload".to_owned()),
                    confidence: None,
                },
            },
        }),
    }
}

pub fn synthetic_subject(i: usize) -> SubjectRef {
    SubjectRef::new(
        Namespace::Arch,
        SubjectKind::Container,
        format!("syn-node-{i:06}"),
    )
    .expect("synthetic subject id is valid")
}

pub fn synthetic_seed() -> String {
    synthetic_subject(0).to_string()
}

/// Statement list for the bulk path over a synthetic log.
pub fn synthetic_statements(events: &[VEvent]) -> Vec<String> {
    let mut statements = Vec::with_capacity(events.len());
    for event in events {
        match &event.payload {
            EventPayload::SubjectDefined(defined) => {
                let properties = Value::Object(defined.properties.clone().into_iter().collect());
                statements.push(subject_upsert_sql(&defined.subject, &properties));
            }
            EventPayload::RelationDeclared(declared) => {
                let fact = &declared.fact;
                statements.push(relation_declare_sql(
                    &fact.relation.from,
                    fact.relation.kind.clone(),
                    &fact.relation.to,
                ));
            }
            _ => {}
        }
    }
    statements
}

/// In-memory N-hop walk on the projected graph, for the latency comparison.
/// Mirrors the native traversal's semantics: follow one relation kind and
/// return the deduplicated set of targets reached at exactly `hops`.
pub fn graph_traversal(
    graph: &SemanticWorldGraph,
    seed: &SubjectRef,
    hops: usize,
    kind: RelationKind,
) -> usize {
    let mut frontier: std::collections::BTreeSet<SubjectRef> =
        std::iter::once(seed.clone()).collect();
    for _ in 0..hops.max(1) {
        let mut next = std::collections::BTreeSet::new();
        for subject in &frontier {
            for fact in graph.outgoing(subject) {
                if fact.relation.kind == kind {
                    next.insert(fact.relation.to.clone());
                }
            }
        }
        frontier = next;
    }
    frontier.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_produces_safe_literals() {
        assert_eq!(surreal_string_literal("a'b"), "'a\\'b'");
        assert_eq!(surreal_string_literal("a\\b"), "'a\\\\b'");
        assert_eq!(
            subject_record("arch:container:x"),
            "subject:['arch:container:x']"
        );
    }

    #[test]
    fn edge_keys_are_deterministic() {
        assert_eq!(
            edge_key("a:b:c", "depends_on", "d:e:f"),
            "a:b:c|depends_on|d:e:f"
        );
    }

    #[test]
    fn synthetic_log_is_shape_stable() {
        let events = synthetic_log(24);
        let subjects = events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::SubjectDefined(_)))
            .count();
        let relations = events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::RelationDeclared(_)))
            .count();
        assert_eq!(subjects, 24);
        // ring edges + chords on multiples of 3, minus the two chord targets
        // that coincide with ring edges.
        assert_eq!(relations, 24 + 8 - 2);
    }

    #[test]
    fn canonical_facts_are_order_insensitive() {
        let a = FactSubject {
            identity: "arch:container:b".to_owned(),
            namespace: "arch".to_owned(),
            kind: "container".to_owned(),
            authority: "authoritative".to_owned(),
            deprecated: false,
            properties: serde_json::json!({"index": 1}),
        };
        let b = FactSubject {
            identity: "arch:container:a".to_owned(),
            ..a.clone()
        };
        let r1 = FactRelation {
            from: "arch:container:a".to_owned(),
            kind: "depends_on".to_owned(),
            to: "arch:container:b".to_owned(),
            authority: "authoritative".to_owned(),
        };
        let digest = facts_digest(&[a.clone(), b.clone()], &[r1.clone()]);
        let digest_swapped = facts_digest(&[b, a], &[r1]);
        assert_eq!(digest, digest_swapped);
    }
}

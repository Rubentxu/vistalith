//! Gate runner for the SurrealDB embedded storage spike (SPK-003).
//!
//! Runs the experiments demanded by the baseline's decision gate
//! (`technology/GRAPH-STORAGE-DECISION.md`) and prints a report:
//!
//! - deterministic migrations: schema applied twice, identical outcome;
//! - deterministic rebuild: the same log replayed into two fresh databases
//!   yields the same digest, and equals the in-memory SWG projection;
//! - rebuild scale: bulk-loaded synthetic facts (statements/second);
//! - traversal latency: native N-hop traversal vs the in-memory graph;
//! - namespace isolation (project-per-database simplicity);
//! - durability + footprint (file engine only: reopen after close, DB size,
//!   binary size).
//!
//! ```text
//! cargo run -p vistalith-spike-surrealdb --bin surrealdb-spike -- --nodes 50000
//! cargo run -p vistalith-spike-surrealdb --features file-engine --bin surrealdb-spike -- \
//!   --engine surrealkv --nodes 50000 --json-out /tmp/spike-report.json
//! ```

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use surrealdb::engine::local::Mem;
use surrealdb::Surreal;
use vistalith_graph::GraphStore;
use vistalith_spike_surrealdb as spike;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Mem,
    #[cfg(feature = "file-engine")]
    SurrealKv,
}

impl Engine {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mem" => Ok(Engine::Mem),
            #[cfg(feature = "file-engine")]
            "surrealkv" => Ok(Engine::SurrealKv),
            #[cfg(not(feature = "file-engine"))]
            "surrealkv" => Err("engine `surrealkv` requires building with \
                `--features file-engine`"
                .to_owned()),
            other => Err(format!("unknown engine `{other}` (mem | surrealkv)")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Engine::Mem => "mem",
            #[cfg(feature = "file-engine")]
            Engine::SurrealKv => "surrealkv",
        }
    }
}

#[derive(Serialize)]
struct GateReport {
    surrealdb_pin: String,
    engine: &'static str,
    nodes: usize,
    subjects: usize,
    relations: usize,
    startup_ms: f64,
    schema_second_application_ms: f64,
    migrations_idempotent: bool,
    rebuild: spike::RebuildStats,
    rebuild_in_memory_ms: f64,
    counts_verified: bool,
    digest_surreal: String,
    digest_in_memory: String,
    digest_matches_in_memory: bool,
    digest_second_replay: String,
    determinism: bool,
    traversal: spike::LatencyStats,
    traversal_in_memory: spike::LatencyStats,
    traversal_reached_surreal: usize,
    traversal_reached_in_memory: usize,
    isolation_ok: bool,
    durability: Option<DurabilityReport>,
    binary_bytes: u64,
    db_bytes: Option<u64>,
}

#[derive(Serialize)]
struct DurabilityReport {
    reopen_ms: f64,
    counts_verified: bool,
    digest_unchanged: bool,
}

#[derive(Serialize)]
struct GateVerdict {
    deterministic_migrations_rebuild: bool,
    acceptable_footprint: bool,
    strong_local_durability: Option<bool>,
    good_traversal_latency: bool,
    no_domain_lock_in: bool,
}

struct Args {
    nodes: usize,
    engine: Engine,
    runs: usize,
    hops: usize,
    batch: usize,
    json_out: Option<PathBuf>,
    keep_db: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        nodes: 50_000,
        engine: Engine::Mem,
        runs: 200,
        hops: 3,
        batch: 1000,
        json_out: None,
        keep_db: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        let mut value = |name: &str| -> Result<String, String> {
            iter.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--nodes" => {
                args.nodes = value("--nodes")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?
            }
            "--engine" => args.engine = Engine::parse(&value("--engine")?)?,
            "--runs" => {
                args.runs = value("--runs")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?
            }
            "--hops" => {
                args.hops = value("--hops")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?
            }
            "--batch" => {
                args.batch = value("--batch")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?
            }
            "--json-out" => args.json_out = Some(PathBuf::from(value("--json-out")?)),
            "--keep-db" => args.keep_db = true,
            other => return Err(format!("unknown flag `{other}`")),
        }
    }
    Ok(args)
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let metadata = entry.metadata();
            if let Ok(metadata) = metadata {
                if metadata.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += metadata.len();
                }
            }
        }
    }
    total
}

fn millis(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("surrealdb-spike: {err}");
            std::process::exit(2);
        }
    };

    if let Err(err) = run_gate(args).await {
        eprintln!("surrealdb-spike: {err:#}");
        std::process::exit(1);
    }
}

async fn run_gate(args: Args) -> Result<(), spike::SpikeError> {
    let started_all = Instant::now();
    println!(
        "surrealdb-spike: pin={} engine={} nodes={} traversal={}x{}hops batch={}",
        env!("CARGO_PKG_NAME"),
        args.engine.name(),
        args.nodes,
        args.runs,
        args.hops,
        args.batch
    );

    let events = spike::synthetic_log(args.nodes);
    let seed = spike::synthetic_seed();
    // Only used by the file-engine path; the mem engine keeps all state in
    // process.
    #[cfg_attr(not(feature = "file-engine"), allow(unused_variables))]
    let db_dir = std::env::temp_dir().join(format!(
        "vistalith-surrealdb-spike-{}",
        std::process::id()
    ));

    // --- startup + schema ---------------------------------------------------
    let startup_started = Instant::now();
    let (db, db_path) = match args.engine {
        Engine::Mem => {
            let db = Surreal::new::<Mem>(()).await?;
            db.use_ns("spike").use_db("spike").await?;
            (db, None::<PathBuf>)
        }
        #[cfg(feature = "file-engine")]
        Engine::SurrealKv => {
            let _ = std::fs::remove_dir_all(&db_dir);
            let db = Surreal::new::<surrealdb::engine::local::SurrealKv>(&*db_dir).await?;
            db.use_ns("spike").use_db("spike").await?;
            (db, Some(db_dir.clone()))
        }
    };
    spike::define_schema(&db).await?;
    let startup_ms = millis(startup_started.elapsed());

    let schema_started = Instant::now();
    spike::define_schema(&db).await?;
    let schema_second_ms = millis(schema_started.elapsed());

    // --- rebuild scale (bulk path) -------------------------------------------
    let statements = spike::synthetic_statements(&events);
    let rebuild = spike::bulk_apply(&db, &statements, args.batch).await?;
    let surreal_subjects = spike::count_rows(&db, "subject").await?;
    let surreal_relations = spike::count_rows(&db, "relates").await?;
    let expected_relations = statements.len() - args.nodes;
    let counts_verified = surreal_subjects == args.nodes && surreal_relations == expected_relations;

    // --- in-memory comparison + domain-boundary digest -----------------------
    let mem_started = Instant::now();
    let store = GraphStore::replay(events.iter().cloned())
        .map_err(|e| spike::SpikeError::Serialization(e.to_string()))?;
    let rebuild_in_memory_ms = millis(mem_started.elapsed());
    let (mem_subjects, mem_relations) = spike::facts_from_graph(store.graph());
    let digest_in_memory = spike::facts_digest(&mem_subjects, &mem_relations);

    let (surreal_subject_facts, surreal_relation_facts) = spike::facts_from_surreal(&db).await?;
    let digest_surreal = spike::facts_digest(&surreal_subject_facts, &surreal_relation_facts);
    let digest_matches_in_memory = digest_surreal == digest_in_memory;

    // --- determinism: second fresh replay ------------------------------------
    let second_db = Surreal::new::<Mem>(()).await?;
    second_db.use_ns("spike").use_db("spike").await?;
    spike::define_schema(&second_db).await?;
    spike::bulk_apply(&second_db, &statements, args.batch).await?;
    let (second_subjects, second_relations) = spike::facts_from_surreal(&second_db).await?;
    let digest_second_replay = spike::facts_digest(&second_subjects, &second_relations);
    let determinism = digest_second_replay == digest_surreal;
    drop(second_db);

    // --- traversal ------------------------------------------------------------
    let traversal = spike::measure_traversal(&db, &seed, args.hops, args.runs).await?;
    let traversal_reached_surreal =
        spike::traverse(&db, &seed, args.hops).await?;
    let mem_seed = spike::synthetic_subject(0);
    let mut mem_samples = Vec::with_capacity(args.runs);
    for _ in 0..args.runs {
        let started = Instant::now();
        spike::graph_traversal(
            store.graph(),
            &mem_seed,
            args.hops,
            vistalith_domain::RelationKind::DependsOn,
        );
        mem_samples.push(started.elapsed());
    }
    let traversal_in_memory = spike::summarize(mem_samples);
    let traversal_reached_in_memory = spike::graph_traversal(
            store.graph(),
            &mem_seed,
            args.hops,
            vistalith_domain::RelationKind::DependsOn,
        );

    // --- namespace isolation ---------------------------------------------------
    db.use_ns("spike-other").use_db("spike").await?;
    let isolated_empty = spike::count_rows(&db, "subject").await? == 0;
    spike::define_schema(&db).await?;
    spike::bulk_apply(
        &db,
        &statements[..statements.len().min(args.batch.max(2))],
        args.batch,
    )
    .await?;
    let isolated_has_rows = spike::count_rows(&db, "subject").await? > 0;
    let isolation_ok = isolated_empty && isolated_has_rows;
    db.use_ns("spike").use_db("spike").await?;

    // --- durability (file engine) ------------------------------------------------
    let durability = match args.engine {
        Engine::Mem => None,
        #[cfg(feature = "file-engine")]
        Engine::SurrealKv => {
            drop(db); // close the storage engine (asynchronous shutdown)
            let reopen_started = Instant::now();
            let reopened = spike::open_surrealkv_with_retry(&db_dir, 10).await?;
            reopened.use_ns("spike").use_db("spike").await?;
            let reopen_ms = millis(reopen_started.elapsed());
            let subjects = spike::count_rows(&reopened, "subject").await?;
            let relations = spike::count_rows(&reopened, "relates").await?;
            let (facts_s, facts_r) = spike::facts_from_surreal(&reopened).await?;
            let digest_reopened = spike::facts_digest(&facts_s, &facts_r);
            Some(DurabilityReport {
                reopen_ms,
                counts_verified: subjects == args.nodes && relations == expected_relations,
                digest_unchanged: digest_reopened == digest_surreal,
            })
        }
    };

    let db_bytes = db_path.as_deref().map(|p| dir_size(p));
    let binary_bytes = std::env::current_exe()
        .ok()
        .and_then(|p| p.metadata().ok())
        .map(|m| m.len())
        .unwrap_or(0);

    let report = GateReport {
        surrealdb_pin: "surrealdb =3.1.6 (newest buildable stable; 3.2.4 pinned \
            diskann 0.54 fails on all stable toolchains tested — rust#100013)"
            .to_owned(),
        engine: args.engine.name(),
        nodes: args.nodes,
        subjects: surreal_subjects,
        relations: surreal_relations,
        startup_ms,
        schema_second_application_ms: schema_second_ms,
        migrations_idempotent: true,
        rebuild,
        rebuild_in_memory_ms,
        counts_verified,
        digest_surreal,
        digest_in_memory,
        digest_matches_in_memory,
        digest_second_replay,
        determinism,
        traversal,
        traversal_in_memory,
        traversal_reached_surreal,
        traversal_reached_in_memory,
        isolation_ok,
        durability,
        binary_bytes,
        db_bytes,
    };

    // The mem engine skips the durability experiment entirely, so the gate
    // cannot pass or fail on it in that mode.
    let verdict = GateVerdict {
        deterministic_migrations_rebuild: report.migrations_idempotent
            && report.determinism
            && report.counts_verified,
        acceptable_footprint: report.startup_ms < 2_000.0
            && report.binary_bytes < 150_000_000,
        strong_local_durability: report.durability.as_ref().map(|d| d.digest_unchanged),
        good_traversal_latency: report.traversal.p95.as_secs_f64() * 1000.0 < 50.0,
        no_domain_lock_in: report.digest_matches_in_memory,
    };

    print_report(&report, &verdict);

    if let Some(path) = args.json_out {
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| spike::SpikeError::Serialization(e.to_string()))?;
        std::fs::write(&path, json)
            .map_err(|e| spike::SpikeError::Serialization(e.to_string()))?;
        println!("json report written to {}", path.display());
    }

    if !args.keep_db {
        if let Some(path) = db_path {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    println!("total: {:.1}s", started_all.elapsed().as_secs_f64());
    Ok(())
}

fn print_report(report: &GateReport, verdict: &GateVerdict) {
    let yes_no = |v: bool| if v { "PASS" } else { "FAIL" };
    println!("\n=== SurrealDB embedded spike report ===");
    println!("pin: {}", report.surrealdb_pin);
    println!(
        "engine: {} | nodes: {} | subjects: {} | relations: {}",
        report.engine, report.nodes, report.subjects, report.relations
    );
    println!("startup (open + schema + first query): {:.1} ms", report.startup_ms);
    println!(
        "schema re-application: {:.1} ms (idempotent: {})",
        report.schema_second_application_ms, report.migrations_idempotent
    );
    println!(
        "rebuild: {} statements in {} queries = {:.0} stmt/s ({:.0} ms) | in-memory replay: {:.1} ms",
        report.rebuild.statements,
        report.rebuild.queries,
        report.rebuild.statements as f64 / report.rebuild.elapsed.as_secs_f64().max(1e-9),
        report.rebuild.elapsed.as_secs_f64() * 1000.0,
        report.rebuild_in_memory_ms
    );
    println!(
        "counts verified: {} | determinism: {} | digest == in-memory: {}",
        yes_no(report.counts_verified),
        yes_no(report.determinism),
        yes_no(report.digest_matches_in_memory)
    );
    println!(
        "traversal ({} runs): surreal p50 {:.0}µs p95 {:.0}µs | in-memory p50 {:.0}µs p95 {:.0}µs | reached s/m: {}/{}",
        report.traversal.runs,
        report.traversal.p50.as_secs_f64() * 1e6,
        report.traversal.p95.as_secs_f64() * 1e6,
        report.traversal_in_memory.p50.as_secs_f64() * 1e6,
        report.traversal_in_memory.p95.as_secs_f64() * 1e6,
        report.traversal_reached_surreal,
        report.traversal_reached_in_memory
    );
    println!("namespace isolation: {}", yes_no(report.isolation_ok));
    match &report.durability {
        Some(d) => println!(
            "durability (reopen after close): {:.1} ms | counts: {} | digest: {}",
            d.reopen_ms,
            yes_no(d.counts_verified),
            yes_no(d.digest_unchanged)
        ),
        None => println!("durability: skipped (mem engine; run --engine surrealkv --features file-engine)"),
    }
    println!(
        "footprint: binary {:.1} MiB | db {}",
        report.binary_bytes as f64 / (1024.0 * 1024.0),
        match report.db_bytes {
            Some(bytes) => format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0)),
            None => "n/a (mem)".to_owned(),
        }
    );
    println!("--- decision gate (technology/GRAPH-STORAGE-DECISION.md) ---");
    println!(
        "deterministic migrations/rebuild: {}",
        yes_no(verdict.deterministic_migrations_rebuild)
    );
    println!("acceptable binary/startup footprint: {}", yes_no(verdict.acceptable_footprint));
    match verdict.strong_local_durability {
        Some(true) => println!("strong local durability: PASS"),
        Some(false) => println!("strong local durability: FAIL"),
        None => println!("strong local durability: n/a (requires file engine)"),
    }
    println!("good traversal latency: {}", yes_no(verdict.good_traversal_latency));
    println!("no lock-in at domain boundary: {}", yes_no(verdict.no_domain_lock_in));
}

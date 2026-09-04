//! Quick interactive probe for SurrealQL syntax questions that come up
//! while tuning the spike (traversal chains, empty-table counts, ns scoping).
//!
//! ```text
//! cargo run --example probe
//! ```

use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;
use vistalith_spike_surrealdb as spike;

#[tokio::main]
async fn main() {
    syntax_probe().await;
    ns_probe().await;
}

async fn syntax_probe() {
    let db: Surreal<Db> = Surreal::new::<Mem>(()).await.expect("open");
    db.use_ns("probe").use_db("probe").await.unwrap();
    spike::define_schema(&db).await.unwrap();

    let events = spike::synthetic_log(8);
    spike::replay_log(&db, &events).await.unwrap();

    let seed = spike::synthetic_seed();
    println!("seed = {seed}");

    for hops in 1..=3 {
        let mut chain = String::new();
        for _ in 0..hops {
            chain.push_str("->relates->subject");
        }
        let sql = format!("SELECT VALUE {chain} FROM subject:['{seed}'];");
        match db.query(&sql).await {
            Ok(mut r) => match r.take::<Vec<serde_json::Value>>(0) {
                Ok(rows) => println!("hops={hops} rows={rows:?}"),
                Err(e) => println!("hops={hops} take ERR {e}"),
            },
            Err(e) => println!("hops={hops} ERR {e}"),
        }
    }

    for sql in [
        "SELECT VALUE count() FROM subject GROUP ALL;",
        "SELECT VALUE count() FROM missing_table GROUP ALL;",
    ] {
        match db.query(sql).await {
            Ok(mut r) => match r.take::<Vec<serde_json::Value>>(0) {
                Ok(v) => println!("{sql} => {v:?}"),
                Err(e) => println!("{sql} take ERR {e}"),
            },
            Err(e) => println!("{sql} ERR {e}"),
        }
    }
}

async fn ns_probe() {
    let db: Surreal<Db> = Surreal::new::<Mem>(()).await.expect("open");
    db.use_ns("a").use_db("d").await.unwrap();
    spike::define_schema(&db).await.unwrap();
    let events = spike::synthetic_log(4);
    spike::replay_log(&db, &events).await.unwrap();
    println!("count in a: {:?}", spike::count_rows(&db, "subject").await);
    db.use_ns("b").use_db("d").await.unwrap();
    println!("raw count in b:");
    match db
        .query("SELECT VALUE count() FROM subject GROUP ALL;")
        .await
    {
        Ok(mut r) => match r.take::<Vec<serde_json::Value>>(0) {
            Ok(v) => println!("  ok {v:?}"),
            Err(e) => println!("  take err: {e}"),
        },
        Err(e) => println!("  query err: {e}"),
    }
    println!(
        "count in b via helper: {:?}",
        spike::count_rows(&db, "subject").await
    );
}

use std::path::PathBuf;

use vistalith_graph::GraphStore;
use vistalith_server::{AppState, router};

struct Args {
    host: String,
    port: u16,
    fixture: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        host: "127.0.0.1".to_owned(),
        port: 7420,
        fixture: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--host" => {
                args.host = iter.next().ok_or("--host needs a value")?;
            }
            "--port" => {
                let value = iter.next().ok_or("--port needs a value")?;
                args.port = value.parse().map_err(|_| format!("bad port: {value}"))?;
            }
            "--fixture" => {
                let value = iter.next().ok_or("--fixture needs a path")?;
                args.fixture = Some(PathBuf::from(value));
            }
            other => {
                return Err(format!(
                    "unknown argument: {other} (usage: vistalithd [--host H] [--port N] [--fixture FILE])"
                ));
            }
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vistalithd=info,tower=warn".into()),
        )
        .init();

    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("vistalithd: {err}");
            std::process::exit(2);
        }
    };

    let store = match &args.fixture {
        Some(path) => match GraphStore::from_fixture_path(path) {
            Ok(store) => {
                tracing::info!(fixture = %path.display(), events = store.log().len(), "fixture loaded");
                store
            }
            Err(err) => {
                eprintln!("vistalithd: cannot load fixture {}: {err}", path.display());
                std::process::exit(1);
            }
        },
        None => GraphStore::new(),
    };

    let app = router(AppState::new(store));
    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("vistalithd: cannot bind {addr}: {err}"));
    tracing::info!("vistalithd listening on http://{addr}");
    axum::serve(listener, app).await.expect("server run loop");
}

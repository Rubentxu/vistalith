use std::path::PathBuf;

use vistalith_agent_runtime::{FakeProvider, RigProvider, RuntimeProvider};
use vistalith_graph::GraphStore;
use vistalith_server::{AppState, router};

struct Args {
    host: String,
    port: u16,
    fixture: Option<PathBuf>,
    provider: String,
    model: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        host: "127.0.0.1".to_owned(),
        port: 7420,
        fixture: None,
        provider: "fake".to_owned(),
        model: "claude-haiku-4-5".to_owned(),
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
            "--provider" => {
                let value = iter.next().ok_or("--provider needs a value")?;
                args.provider = value.to_lowercase();
            }
            "--model" => {
                args.model = iter.next().ok_or("--model needs a value")?;
            }
            other => {
                return Err(format!(
                    "unknown argument: {other} (usage: vistalithd [--host H] [--port N] \
                     [--fixture FILE] [--provider fake|anthropic] [--model NAME])"
                ));
            }
        }
    }
    Ok(args)
}

fn build_runtime(args: &Args) -> Result<RuntimeProvider, String> {
    match args.provider.as_str() {
        "fake" => Ok(RuntimeProvider::Fake(FakeProvider::repeating(
            "[fake provider] offline reply — run with --provider anthropic for a live model",
        ))),
        "anthropic" => {
            // SPEC-008: credentials are read once here and never returned to
            // any renderer surface.
            let key = std::env::var("VISTALITH_ANTHROPIC_API_KEY")
                .map_err(|_| "--provider anthropic needs VISTALITH_ANTHROPIC_API_KEY".to_owned())?;
            RigProvider::anthropic(key, args.model.clone())
                .map(RuntimeProvider::Rig)
                .map_err(|e| e.to_string())
        }
        other => Err(format!(
            "unknown provider `{other}` (available: fake, anthropic)"
        )),
    }
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
                tracing::info!(
                    fixture = %path.display(),
                    events = store.log().len(),
                    "fixture loaded"
                );
                store
            }
            Err(err) => {
                eprintln!("vistalithd: cannot load fixture {}: {err}", path.display());
                std::process::exit(1);
            }
        },
        None => GraphStore::new(),
    };

    let runtime = match build_runtime(&args) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("vistalithd: {err}");
            std::process::exit(2);
        }
    };
    tracing::info!(provider = %runtime.descriptor(), "conversation runtime ready");

    let app = router(AppState::with_runtime(store, runtime));
    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("vistalithd: cannot bind {addr}: {err}"));
    tracing::info!("vistalithd listening on http://{addr}");
    axum::serve(listener, app).await.expect("server run loop");
}

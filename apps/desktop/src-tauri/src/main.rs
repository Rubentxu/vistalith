//! Vistalith desktop shell (Tauri 2, slice 5).
//!
//! B4 (TypeScript owns human experience) still holds: the window loads the
//! same React web lens that the browser uses. The shell only adds
//! local-first conveniences around it — it can detect the `vistalithd`
//! backend and, when asked, launch one next to the app. The shell is never
//! an authority: it owns no semantic state and all truth stays behind the
//! `vistalithd` API.

use serde::Serialize;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

/// Default local backend (matches `apps/web/src/api.ts` and `vistalithd`'s
/// default port).
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7420;
const HEALTH_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BackendStatus {
    pub url: String,
    pub online: bool,
    /// Graph revision reported by `/health` when online.
    pub revision: Option<u64>,
    pub events: Option<u64>,
}

/// `(host, port, base-url)` of the configured backend. `VISTALITHD_URL` may
/// override the default `http://127.0.0.1:7420` (scheme accepted, only
/// host:port are used — the backend is local by construction).
pub fn backend_address() -> (String, u16, String) {
    let raw = std::env::var("VISTALITHD_URL")
        .unwrap_or_else(|_| format!("http://{DEFAULT_HOST}:{DEFAULT_PORT}"));
    parse_backend_address(&raw)
}

pub fn parse_backend_address(raw: &str) -> (String, u16, String) {
    let authority = raw
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_owned();
    if authority.is_empty() {
        return (
            DEFAULT_HOST.to_owned(),
            DEFAULT_PORT,
            format!("http://{DEFAULT_HOST}:{DEFAULT_PORT}"),
        );
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(port) => (host.to_owned(), port),
            Err(_) => (authority.clone(), DEFAULT_PORT),
        },
        None => (authority.clone(), DEFAULT_PORT),
    };
    (host.clone(), port, format!("http://{host}:{port}"))
}

/// One minimal HTTP GET against `/health`, parsed without pulling an HTTP
/// client into the shell. Returns `(revision, events)` when the backend
/// answers with its status document.
pub fn probe_health(host: &str, port: u16, url: &str) -> Option<(u64, u64)> {
    let mut stream = TcpStream::connect((host, port)).ok()?;
    stream.set_read_timeout(Some(HEALTH_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(HEALTH_TIMEOUT)).ok()?;
    let request = format!("GET {url}/health HTTP/1.1\r\nhost: {host}:{port}\r\nconnection: close\r\naccept: application/json\r\n\r\n");
    use std::io::{Read, Write};
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let body = response.split("\r\n\r\n").nth(1)?;
    let revision = extract_u64_field(body, "graph_revision")?;
    let events = extract_u64_field(body, "events")?;
    Some((revision, events))
}

/// Extracts an integer field from a JSON body without a full parser. The
/// health document is small and flat (`/health` contract), so a targeted
/// scan is enough for the shell's indicator.
fn extract_u64_field(body: &str, field: &str) -> Option<u64> {
    let needle = format!("\"{field}\"");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let colon = rest.find(':')?;
    let digits: String = rest[colon + 1..]
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

pub fn backend_status() -> BackendStatus {
    let (host, port, url) = backend_address();
    match probe_health(&host, port, &url) {
        Some((revision, events)) => BackendStatus {
            url,
            online: true,
            revision: Some(revision),
            events: Some(events),
        },
        None => BackendStatus {
            url,
            online: false,
            revision: None,
            events: None,
        },
    }
}

/// Locates a `vistalithd` binary to launch: `VISTALITHD_BIN` first, then
/// next to the running shell, then the `PATH`.
pub fn find_backend_binary() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("VISTALITHD_BIN") {
        let path = PathBuf::from(custom);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        let candidate = dir.join("vistalithd");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("vistalithd"))
        .find(|candidate| candidate.is_file())
}

/// Ensures a backend is running: reports the healthy one, or launches a
/// `vistalithd` and waits briefly for it to come up. The shell does not
/// block the UI on this — the lens renders its offline banner either way.
pub fn ensure_backend() -> BackendStatus {
    let status = backend_status();
    if status.online {
        return status;
    }
    let Some(binary) = find_backend_binary() else {
        return status;
    };
    let spawned = std::process::Command::new(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if spawned.is_err() {
        return status;
    }
    // The child is intentionally leaked: the backend must outlive the shell
    // process so the browser lens keeps working too.
    let (host, port, url) = backend_address();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(150));
        if let Some((revision, events)) = probe_health(&host, port, &url) {
            return BackendStatus {
                url,
                online: true,
                revision: Some(revision),
                events: Some(events),
            };
        }
    }
    status
}

#[tauri::command]
fn backend_health() -> BackendStatus {
    backend_status()
}

#[tauri::command]
fn backend_start() -> BackendStatus {
    ensure_backend()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![backend_health, backend_start])
        .run(tauri::generate_context!())
        .expect("error while running the Vistalith desktop shell");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn backend_address_defaults_to_local_7420() {
        let (host, port, url) = parse_backend_address("http://127.0.0.1:7420");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 7420);
        assert_eq!(url, "http://127.0.0.1:7420");
        // Default shape (no env override): documented local endpoint.
        if std::env::var_os("VISTALITHD_URL").is_none() {
            assert_eq!(backend_address(), parse_backend_address(""));
        }
    }

    #[test]
    fn backend_address_parses_overrides() {
        assert_eq!(
            parse_backend_address("http://localhost:9000").1,
            9000,
            "explicit ports win"
        );
        assert_eq!(parse_backend_address("localhost:9000").1, 9000);
        assert_eq!(
            parse_backend_address("localhost").1,
            DEFAULT_PORT,
            "missing ports fall back to the default"
        );
        // The extraction helper on synthetic health documents.
        assert_eq!(extract_u64_field(r#"{"graph_revision": 7}"#, "graph_revision"), Some(7));
        assert_eq!(extract_u64_field(r#"{"events": 12}"#, "events"), Some(12));
        assert_eq!(extract_u64_field(r#"{"events": null}"#, "events"), None);
        assert_eq!(extract_u64_field("{}", "graph_revision"), None);
    }

    #[test]
    fn probe_health_parses_a_live_backend() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 2048];
            let _ = stream.read(&mut buffer);
            let response = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\",\"service\":\"vistalithd\",\"graph_revision\":9,\"events\":15,\"provider\":\"fake\"}";
            stream.write_all(response.as_bytes()).unwrap();
        });

        let (revision, events) =
            probe_health("127.0.0.1", port, "http://127.0.0.1").expect("backend answers");
        assert_eq!(revision, 9);
        assert_eq!(events, 15);
        server.join().unwrap();
    }

    #[test]
    fn probe_health_reports_offline_backends() {
        // Port 1 on loopback is closed in normal environments.
        let result = probe_health("127.0.0.1", 1, "http://127.0.0.1");
        assert!(result.is_none(), "closed ports must read as offline");
    }
}

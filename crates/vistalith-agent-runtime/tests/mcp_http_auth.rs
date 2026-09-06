//! SPK-007 end-to-end: an authenticated remote MCP server over Streamable
//! HTTP. A real rmcp `StreamableHttpService` sits behind an auth gate; the
//! Vistalith client must carry the credential on EVERY request (the
//! handshake discovers tools immediately, so a connection succeeding means
//! `initialize` + `tools/list` both authenticated). Secrets must never
//! appear in statuses or errors.

use std::convert::Infallible;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use rmcp::ErrorData as McpServerError;
use rmcp::model::{JsonObject, ListToolsResult, ServerInfo, Tool, ToolAnnotations};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServerHandler;
use tower_service::Service;
use vistalith_agent_runtime::{ConnectionStatus, McpAuth, McpConnection, McpError, McpServerConfig};

const SECRET: &str = "super-secret-bearer-token";
const API_KEY: &str = "key-1234567890";

fn message_schema() -> Arc<JsonObject> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "message": { "type": "string" } },
        "required": ["message"]
    });
    schema.as_object().expect("object schema").clone().into()
}

struct EchoServer;

impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(rmcp::model::ServerCapabilities::default())
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpServerError> {
        let echo = Tool::new("echo", "Echoes the message.", message_schema())
            .with_annotations(ToolAnnotations::from_raw(
                Some("Echo".to_owned()),
                Some(true),
                None,
                None,
                None,
            ));
        Ok(ListToolsResult {
            tools: vec![echo],
            ..Default::default()
        })
    }
}

/// Spawns an rmcp Streamable HTTP server behind the given auth predicate on
/// an ephemeral loopback port and returns its MCP endpoint URL.
async fn spawn_server<G>(allows: G) -> String
where
    G: Fn(&HeaderMap) -> bool + Clone + Send + Sync + 'static,
{
    let rmcp_service = StreamableHttpService::new(
        || Ok(EchoServer),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new().fallback_service(GateService {
        allows,
        inner: rmcp_service,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    format!("http://127.0.0.1:{port}/mcp")
}

/// Tower middleware: only requests passing the predicate reach the rmcp
/// service; everything else gets 401 before the MCP layer sees them.
#[derive(Clone)]
struct GateService<G, S> {
    allows: G,
    inner: S,
}

impl<G, S> Service<Request<Body>> for GateService<G, S>
where
    G: Fn(&HeaderMap) -> bool + Clone + Send + Sync + 'static,
    S: Service<Request<Body>, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Response: IntoResponse,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, Infallible>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let allowed = (self.allows)(request.headers());
        if !allowed {
            let response = (
                StatusCode::UNAUTHORIZED,
                "missing or invalid credentials",
            )
                .into_response();
            return Box::pin(async move { Ok(response) });
        }
        let future = self.inner.call(request);
        Box::pin(async move {
            let response = future.await?;
            Ok(response.into_response())
        })
    }
}

fn bearer_gate(token: &'static str) -> impl Fn(&HeaderMap) -> bool + Clone + Send + Sync {
    let expected = format!("Bearer {token}");
    move |headers: &HeaderMap| {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some(expected.as_str())
    }
}

#[tokio::test]
async fn bearer_auth_connects_and_discovers_tools() {
    let url = spawn_server(bearer_gate(SECRET)).await;
    let config = McpServerConfig {
        name: "secure-echo".to_owned(),
        command: None,
        args: vec![],
        url: Some(url),
        auth: Some(McpAuth::Bearer {
            token: Some(SECRET.to_owned()),
            token_env: None,
        }),
    };
    let connection = McpConnection::connect(config).await.expect("connect");
    let status = connection.status();
    assert_eq!(status.status, ConnectionStatus::Connected);
    assert_eq!(status.tools, 1);
    // redacted status: the kind travels, the secret never does
    assert_eq!(status.auth.as_deref(), Some("bearer"));
    let status_json = serde_json::to_string(&status).expect("serialize");
    assert!(!status_json.contains(SECRET), "token leaked in status");
}

#[tokio::test]
async fn missing_or_wrong_credentials_fail_to_connect() {
    let url = spawn_server(bearer_gate(SECRET)).await;
    for auth in [
        None,
        Some(McpAuth::Bearer {
            token: Some("wrong-token".to_owned()),
            token_env: None,
        }),
    ] {
        let config = McpServerConfig {
            name: "secure-echo".to_owned(),
            command: None,
            args: vec![],
            url: Some(url.clone()),
            auth,
        };
        let error = McpConnection::connect(config).await;
        assert!(
            matches!(error, Err(McpError::Connect(_, _))),
            "unauthenticated connect must fail"
        );
    }
}

#[tokio::test]
async fn header_auth_connects() {
    let url = spawn_server(move |headers: &HeaderMap| {
        headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            == Some(API_KEY)
    })
    .await;
    let config = McpServerConfig {
        name: "keyed-echo".to_owned(),
        command: None,
        args: vec![],
        url: Some(url),
        auth: Some(McpAuth::Header {
            name: "x-api-key".to_owned(),
            value: Some(API_KEY.to_owned()),
            value_env: None,
        }),
    };
    let connection = McpConnection::connect(config).await.expect("connect");
    assert_eq!(connection.status().tools, 1);
    assert_eq!(
        connection.status().auth.as_deref(),
        Some("header:x-api-key")
    );
}

#[tokio::test]
async fn reconnect_carries_the_credential() {
    let url = spawn_server(bearer_gate(SECRET)).await;
    let config = McpServerConfig {
        name: "secure-echo".to_owned(),
        command: None,
        args: vec![],
        url: Some(url),
        auth: Some(McpAuth::Bearer {
            token: Some(SECRET.to_owned()),
            token_env: None,
        }),
    };
    let connection = McpConnection::connect(config).await.expect("connect");
    // reconnect re-opens the HTTP session from the same config: the fresh
    // handshake must authenticate again
    connection.reconnect().await.expect("reconnect");
    assert_eq!(connection.status().tools, 1);
}

#[tokio::test]
async fn env_referenced_token_resolves() {
    let url = spawn_server(bearer_gate(SECRET)).await;
    // SAFETY: test-only setup with a unique variable name; no other test
    // reads it.
    unsafe { std::env::set_var("VISTALITH_TEST_MCP_TOKEN", SECRET) };
    let config = McpServerConfig {
        name: "env-echo".to_owned(),
        command: None,
        args: vec![],
        url: Some(url),
        auth: Some(McpAuth::Bearer {
            token: None,
            token_env: Some("VISTALITH_TEST_MCP_TOKEN".to_owned()),
        }),
    };
    let connection = McpConnection::connect(config).await.expect("connect");
    assert_eq!(connection.status().tools, 1);

    // a missing env variable is a connect error naming the variable —
    // never the secret
    let url = spawn_server(bearer_gate(SECRET)).await;
    let config = McpServerConfig {
        name: "env-echo".to_owned(),
        command: None,
        args: vec![],
        url: Some(url),
        auth: Some(McpAuth::Bearer {
            token: None,
            token_env: Some("VISTALITH_TEST_MCP_MISSING".to_owned()),
        }),
    };
    let error = match McpConnection::connect(config).await {
        Err(error) => error,
        Ok(_) => panic!("connect with missing env token must fail"),
    };
    assert!(error.to_string().contains("VISTALITH_TEST_MCP_MISSING"));
    assert!(!error.to_string().contains(SECRET));
}

#[test]
fn auth_on_stdio_is_rejected() {
    let config = McpServerConfig {
        name: "stdio-auth".to_owned(),
        command: Some("echo".to_owned()),
        args: vec![],
        url: None,
        auth: Some(McpAuth::Bearer {
            token: Some(SECRET.to_owned()),
            token_env: None,
        }),
    };
    assert!(config.validate().is_err());
}

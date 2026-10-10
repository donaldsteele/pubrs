//! Streamable HTTP transport: a raw HTTP/1.1 initialize request is answered.

use pubrs_api::{Session, SessionConfig};
use pubrs_mcp::{PubrsServer, http_router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_initialize_and_host_check() {
    let server = PubrsServer::new(Session::new(SessionConfig::default()));
    let ct = CancellationToken::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = http_router(server, ct.child_token());
    let stop = ct.clone();
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { stop.cancelled_owned().await })
            .await
            .unwrap();
    });
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
    let request = |host: &str| {
        format!(
            "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let send = |req: String| async move {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(10), s.read_to_end(&mut buf)).await;
        String::from_utf8_lossy(&buf).into_owned()
    };
    let ok = send(request(&addr.to_string())).await;
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    assert!(ok.contains("\"name\":\"pubrs\""), "{ok}");
    let forbidden = send(request("evil.example.com")).await;
    assert!(forbidden.starts_with("HTTP/1.1 403"), "{forbidden}");
    ct.cancel();
}

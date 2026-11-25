//! Local HTTP stub for tests. Separate from `test_support`, which needs `App` and so isn't in the
//! library target.
#![allow(dead_code)]

/// Local HTTP server answering every request with `body`
pub(crate) async fn spawn_json_stub(body: &'static str) -> String {
    spawn_stub("200 OK", body).await.0
}

/// Local HTTP server answering every request with `status` and `body`. The receiver yields
/// each request line.
pub(crate) async fn spawn_stub(
    status: &'static str,
    body: &'static str,
) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let _ = tx.send(request.lines().next().unwrap_or("").to_string());
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = sock.write_all(response.as_bytes()).await;
            });
        }
    });
    (format!("http://{}", addr), rx)
}

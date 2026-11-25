use anyhow::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorType {
    ConnectionRefused,
    Timeout,
    Unauthorized, // HTTP 401
    NotFound,     // HTTP 404
    ServerError,  // HTTP 500+
    NetworkError, // DNS, routing, etc.
    Other,
}

/// Classify an error based on its type and error chain
pub fn classify_error(error: &Error) -> ErrorType {
    let error_msg = error.to_string().to_lowercase();

    // Check for connection-specific errors
    if error_msg.contains("connection refused") {
        return ErrorType::ConnectionRefused;
    }
    if error_msg.contains("timeout") || error_msg.contains("timed out") {
        return ErrorType::Timeout;
    }

    // Check for HTTP status codes (via reqwest error chain)
    if let Some(reqwest_err) = error.downcast_ref::<reqwest::Error>()
        && let Some(status) = reqwest_err.status()
    {
        return match status.as_u16() {
            401 => ErrorType::Unauthorized,
            404 => ErrorType::NotFound,
            500..=599 => ErrorType::ServerError,
            _ => ErrorType::Other,
        };
    }

    // Network-level errors
    if error_msg.contains("dns") || error_msg.contains("network") {
        return ErrorType::NetworkError;
    }

    ErrorType::Other
}

/// Format error message for tech-savvy audience - show raw error details
pub fn format_error_message(error: &Error) -> String {
    // Walk the error chain to find reqwest::Error (most informative for network errors)
    let mut current: Option<&dyn std::error::Error> = Some(error.as_ref());

    while let Some(err) = current {
        if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>() {
            return reqwest_err.to_string();
        }
        current = err.source();
    }

    // If no reqwest error found, walk the chain to get the deepest (root cause) error
    let mut source = error.source();
    let mut deepest = error.to_string();

    while let Some(err) = source {
        deepest = err.to_string();
        source = err.source();
    }

    deepest
}

/// The cause in a few words, for places with little room
pub fn short_error_message(error: &Error) -> String {
    let reqwest_error = error
        .chain()
        .find_map(|e| e.downcast_ref::<reqwest::Error>());
    match reqwest_error {
        Some(e) if e.is_timeout() => "timed out".to_string(),
        Some(e) if e.is_connect() => "can't connect".to_string(),
        Some(e) if e.status().is_some() => {
            format!("HTTP {}", e.status().unwrap_or_default())
        }
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_short_error_message_leads_with_the_cause() {
        use anyhow::Context;
        let (url, _rx) = crate::test_http::spawn_stub("403 Forbidden", "{}").await;
        let refused = reqwest::get(&url)
            .await
            .unwrap()
            .error_for_status()
            .context("Failed")
            .unwrap_err();
        assert_eq!(short_error_message(&refused), "HTTP 403 Forbidden");

        let unreachable = reqwest::get("http://127.0.0.1:9")
            .await
            .context("Failed")
            .unwrap_err();
        assert_eq!(short_error_message(&unreachable), "can't connect");

        // Connects but never answers
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let slow = reqwest::Client::new()
            .get(format!("http://{}", silent.local_addr().unwrap()))
            .timeout(std::time::Duration::from_millis(50))
            .send()
            .await
            .context("Failed")
            .unwrap_err();
        assert_eq!(short_error_message(&slow), "timed out");

        assert_eq!(short_error_message(&anyhow::anyhow!("boom")), "boom");
        let nested = anyhow::anyhow!("bad json").context("Failed to parse");
        assert_eq!(short_error_message(&nested), "Failed to parse");
    }

    #[test]
    fn test_classify_connection_refused() {
        let err = anyhow::anyhow!("connection refused (os error 111)");
        assert_eq!(classify_error(&err), ErrorType::ConnectionRefused);
    }

    #[test]
    fn test_classify_connection_refused_uppercase() {
        let err = anyhow::anyhow!("Connection Refused");
        assert_eq!(classify_error(&err), ErrorType::ConnectionRefused);
    }

    #[test]
    fn test_classify_timeout() {
        let err = anyhow::anyhow!("request timed out");
        assert_eq!(classify_error(&err), ErrorType::Timeout);
    }

    #[test]
    fn test_classify_timeout_variant() {
        let err = anyhow::anyhow!("operation timeout");
        assert_eq!(classify_error(&err), ErrorType::Timeout);
    }

    #[test]
    fn test_classify_dns_error() {
        let err = anyhow::anyhow!("dns lookup failed");
        assert_eq!(classify_error(&err), ErrorType::NetworkError);
    }

    #[test]
    fn test_classify_network_error() {
        let err = anyhow::anyhow!("network unreachable");
        assert_eq!(classify_error(&err), ErrorType::NetworkError);
    }

    #[test]
    fn test_classify_other_error() {
        let err = anyhow::anyhow!("some random error");
        assert_eq!(classify_error(&err), ErrorType::Other);
    }

    #[test]
    fn test_format_shows_raw_error() {
        let err = anyhow::anyhow!("connection refused");
        let msg = format_error_message(&err);
        assert_eq!(msg, "connection refused");
    }

    #[test]
    fn test_format_shows_root_cause() {
        // Simulate anyhow context wrapping - should extract root cause
        let inner = anyhow::anyhow!("tcp connect error");
        let outer = inner.context("Failed to fetch system config");
        let msg = format_error_message(&outer);
        // Should show the root cause, not the context wrapper
        assert_eq!(msg, "tcp connect error");
    }

    #[test]
    fn test_format_preserves_simple_errors() {
        let err = anyhow::anyhow!("custom error message");
        let msg = format_error_message(&err);
        assert_eq!(msg, "custom error message");
    }
}

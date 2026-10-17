// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read-only, on-demand checks for native Studio integration.
//! A reachable JSON endpoint is not proof that generation or auth is ready.

use std::io::Read;
use std::time::Duration;

/// Transport results deliberately contain no response bodies, tokens or URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reachability {
    /// A bounded JSON object was returned successfully.
    Reachable,
    /// The service refused, timed out, redirected, or returned an HTTP error.
    Unavailable,
    /// A successful response was not a bounded JSON object.
    InvalidResponse,
}

/// Check one local service. Only a port and static health route are accepted;
/// callers cannot send credentials or contact arbitrary remote origins here.
/// Call on a worker, never on the UI thread. No redirect is followed.
pub fn check_local(port: u16, route: &str) -> Reachability {
    check_with_timeout(port, route, Duration::from_secs(3))
}

fn check_with_timeout(port: u16, route: &str, timeout: Duration) -> Reachability {
    if port == 0
        || !matches!(
            route,
            "/health" | "/health/ready" | "/api/health/ping" | "/v1/local/browser/profiles"
        )
    {
        return Reachability::InvalidResponse;
    }
    check_url(&format!("http://127.0.0.1:{port}{route}"), timeout)
}

/// Reach the configured cloud backend, not a hardcoded localhost port. Sends
/// no credential and never follows redirects. Health is not authentication.
pub fn check_backend(origin: &str) -> Reachability {
    let Ok(url) = url::Url::parse(origin) else {
        return Reachability::InvalidResponse;
    };
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !(local && url.scheme() == "http"))
        || url.host().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Reachability::InvalidResponse;
    }
    check_url(
        &format!("{}/health", origin.trim_end_matches('/')),
        Duration::from_secs(5),
    )
}

fn check_url(address: &str, timeout: Duration) -> Reachability {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .build();
    let Ok(response) = agent.get(address).set("Accept", "application/json").call() else {
        return Reachability::Unavailable;
    };
    if response.status() != 200 {
        return Reachability::Unavailable;
    }
    let mut bytes = Vec::new();
    if response
        .into_reader()
        .take(65537)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 65536
    {
        return Reachability::InvalidResponse;
    }
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) if value.is_object() => Reachability::Reachable,
        _ => Reachability::InvalidResponse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fixture(response: String) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("fixture request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("timeout");
            let mut request = [0; 4096];
            let size = stream.read(&mut request).expect("read request");
            let text = String::from_utf8_lossy(&request[..size]);
            assert!(text.starts_with("GET /health HTTP/1.1"));
            assert!(!text.to_lowercase().contains("authorization:"));
            // An oversize client may close before the fixture finishes writing.
            let _ = stream.write_all(response.as_bytes());
        });
        (port, worker)
    }

    fn response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn accepts_json_objects_without_claiming_engine_readiness() {
        let (port, worker) = fixture(response("200 OK", r#"{"status":"degraded"}"#));
        assert_eq!(check_local(port, "/health"), Reachability::Reachable);
        worker.join().expect("fixture finished");
    }
    #[test]
    fn configured_backend_health_is_not_forced_to_localhost_or_sent_credentials() {
        let (port, worker) = fixture(response("200 OK", r#"{"status":"ok"}"#));
        assert_eq!(
            check_backend(&format!("http://127.0.0.1:{port}")),
            Reachability::Reachable
        );
        worker.join().unwrap();
        for origin in [
            "http://public.example",
            "https://user:secret@example.test",
            "https://example.test/private",
            "https://example.test?token=secret",
        ] {
            assert_eq!(check_backend(origin), Reachability::InvalidResponse);
        }
    }

    #[test]
    fn rejects_html_arrays_and_oversize_bodies() {
        for body in [
            "<html>login</html>".to_owned(),
            "[]".to_owned(),
            "x".repeat(65537),
        ] {
            let (port, worker) = fixture(response("200 OK", &body));
            assert_eq!(check_local(port, "/health"), Reachability::InvalidResponse);
            worker.join().expect("fixture finished");
        }
    }

    #[test]
    fn rejects_errors_and_does_not_follow_redirects() {
        for status in ["401 Unauthorized", "503 Service Unavailable"] {
            let (port, worker) = fixture(response(status, "{}"));
            assert_eq!(check_local(port, "/health"), Reachability::Unavailable);
            worker.join().expect("fixture finished");
        }
        let (port, worker) = fixture("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/health\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        assert_eq!(check_local(port, "/health"), Reachability::Unavailable);
        worker.join().expect("fixture finished");
    }

    #[test]
    fn refuses_arbitrary_routes_without_network_access() {
        assert_eq!(check_local(1, "/generate"), Reachability::InvalidResponse);
        assert_eq!(check_local(0, "/health"), Reachability::InvalidResponse);
    }

    #[test]
    fn times_out_on_a_silent_local_service() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("request");
            let _ = held.recv_timeout(Duration::from_secs(2));
        });
        assert_eq!(
            check_with_timeout(port, "/health", Duration::from_millis(40)),
            Reachability::Unavailable
        );
        let _ = release.send(());
        worker.join().expect("fixture finished");
    }
}

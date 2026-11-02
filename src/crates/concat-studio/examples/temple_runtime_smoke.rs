//! Explicit local smoke check; run only with a test origin configured.
fn main() {
    let origin = std::env::var("CONCAT_TEMPLE_URL").expect("set an isolated test origin");
    let endpoint = concat_studio::temple_live::Endpoint::parse(&origin).unwrap();
    let first = match concat_studio::temple_runtime::ensure(&endpoint) {
        Ok(started) => started,
        Err(error) => {
            // Startup may outlive the launcher's soft deadline on Windows.
            // Verify the actual service, rather than treating a pending child
            // as ready or spawning repeated copies.
            use std::io::{Read, Write};
            let address = endpoint.origin.strip_prefix("http://").expect("local HTTP smoke origin");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let mut ready = false;
            while std::time::Instant::now() < deadline {
                if let Ok(mut socket) = std::net::TcpStream::connect(address) {
                    let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                    let _ = write!(socket, "GET /status HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
                    let mut response = String::new();
                    let _ = socket.take(64 * 1024).read_to_string(&mut response);
                    if response.starts_with("HTTP/1.1 200") && response.contains("\"runtime\":\"concat-temple-live\"") && response.contains("\"running\":true") { ready = true; break; }
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            assert!(ready, "runtime did not become ready: {error}");
            println!("TEMPLE_RUNTIME_PENDING_START_VERIFIED=true");
            true
        }
    };
    assert!(
        !concat_studio::temple_runtime::ensure(&endpoint).unwrap(),
        "must reuse one running service"
    );
    println!("TEMPLE_RUNTIME_SMOKE=PASS started={first} reused=true origin={origin}");
}

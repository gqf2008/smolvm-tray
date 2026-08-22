// Cross-platform port liveness probes, std-only.
//
// Why HTTP instead of connect-only: smolvm's `-p` is a published-TCP forwarder —
// the host keeps 127.0.0.1:PORT listening even when the guest service is dead,
// so a bare connect probe reports "up" with a dead backend (e.g. socat down,
// kite mid-crash-respawn). An HTTP/1.0 header read proves the whole chain:
// forwarder + guest service answering.
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

/// Connect-only fallback probe (`SMOLVM_TRAY_PORT_PROBE=tcp`).
pub fn tcp_probe(port: u16, budget: Duration) -> bool {
    let addr = format!("127.0.0.1:{port}");
    TcpStream::connect_timeout(&addr.parse().unwrap(), budget).is_ok()
}

/// Send `GET path HTTP/1.1`, read only until the header terminator, and return
/// true if the response starts with `HTTP/`. Any status code counts — an HTTP
/// answer proves the service (or its proxy) is alive on the other end.
/// Never holds the connection open (SSE endpoints stay open after headers).
///
/// MUST be HTTP/1.1 with Connection: close: chromium's DevTools server answers
/// HTTP/1.0 requests with an empty response (verified on build 151).
pub fn http_probe(port: u16, path: &str, budget: Duration) -> bool {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = match TcpStream::connect_timeout(&addr.parse().unwrap(), budget) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(budget));
    let _ = stream.set_write_timeout(Some(budget));
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut got = Vec::with_capacity(512);
    let mut buf = [0u8; 64];
    while got.len() < 512 {
        match stream.read(&mut buf) {
            Ok(0) => break, // EOF before headers — dead
            Ok(n) => {
                got.extend_from_slice(&buf[..n]);
                if found_header_end(&got) {
                    break;
                }
            }
            Err(_) => break, // timeout or reset
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    got.starts_with(b"HTTP/")
}

fn found_header_end(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn serve_one(response: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(response);
                // hold the socket open briefly to emulate an SSE endpoint
                let _ = s.write_all(b"xx");
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        port
    }

    #[test]
    fn http_ok() {
        assert!(http_probe(serve_one(b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<html>"), "/", Duration::from_millis(800)));
    }

    #[test]
    fn http_non_200_still_counts() {
        assert!(http_probe(serve_one(b"HTTP/1.0 404 Not Found\r\n\r\n"), "/", Duration::from_millis(800)));
    }

    #[test]
    fn http_junk_is_dead() {
        assert!(!http_probe(serve_one(b"this is not http at all\r\n\r\n"), "/", Duration::from_millis(800)));
    }

    #[test]
    fn closed_port_is_down() {
        // Bind then drop — a port nothing listens on.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(!http_probe(port, "/", Duration::from_millis(300)));
        assert!(!tcp_probe(port, Duration::from_millis(300)));
    }

    #[test]
    fn slow_server_respects_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 128];
                let _ = s.read(&mut buf);
                std::thread::sleep(Duration::from_millis(2000));
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\n");
            }
        });
        let start = std::time::Instant::now();
        assert!(!http_probe(port, "/", Duration::from_millis(300)));
        assert!(start.elapsed() < Duration::from_millis(1500));
    }
}

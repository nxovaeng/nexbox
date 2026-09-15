//! A local HTTP proxy that forwards to the core's SOCKS5 listener.
//!
//! Windows needs this. WinINET can be told about a SOCKS proxy with
//! `socks=host:port`, but its SOCKS support is version 4, and Chrome and Edge
//! ignore that key from system settings altogether — so pointing the system
//! proxy at Aether's SOCKS5 listener sets a value almost nothing obeys. What
//! Windows applications do follow is an HTTP proxy, which is what this serves,
//! translating each request into a SOCKS5 connection.
//!
//! Deliberately std-only and thread-per-connection: this crate supervises a
//! child process and has no async runtime, and a desktop client's own traffic
//! does not justify adding one.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Long enough for a slow tunnel to answer, short enough that a wedged peer
/// does not hold a thread forever.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// A request line plus headers. Anything larger is not a proxy request.
const MAX_HEADER_BYTES: usize = 32 * 1024;

pub struct HttpBridge {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl HttpBridge {
    /// Where to point the system proxy.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock the accept loop, which is parked in accept() rather than
        // polling a flag it cannot see.
        let _ = TcpStream::connect(self.address);
    }
}

impl Drop for HttpBridge {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Binds on loopback and forwards everything to `socks`.
///
/// Port 0 lets the OS choose, so two instances cannot collide and nothing has
/// to be configured.
pub fn start(socks: SocketAddr) -> io::Result<HttpBridge> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let address = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));

    let accept_stop = stop.clone();
    thread::Builder::new()
        .name("whiteaesther-http-bridge".into())
        .spawn(move || {
            for client in listener.incoming() {
                if accept_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(client) = client else { continue };
                thread::spawn(move || {
                    // A failed exchange closes that connection and nothing else.
                    let _ = serve(client, socks);
                });
            }
        })?;

    Ok(HttpBridge { address, stop })
}

fn serve(mut client: TcpStream, socks: SocketAddr) -> io::Result<()> {
    client.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut reader = BufReader::new(client.try_clone()?);

    let mut request_line = String::new();
    if read_line(&mut reader, &mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(method), Some(target)) => (method.to_string(), target.to_string()),
        _ => return respond(&mut client, 400, "Bad Request"),
    };

    let mut headers = Vec::new();
    let mut total = request_line.len();
    loop {
        let mut line = String::new();
        if read_line(&mut reader, &mut line)? == 0 {
            break;
        }
        total += line.len();
        if total > MAX_HEADER_BYTES {
            return respond(&mut client, 431, "Request Header Fields Too Large");
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        headers.push(line);
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        // https, and anything else tunnelled. The overwhelming majority.
        let Some((host, port)) = split_authority(&target, 443) else {
            return respond(&mut client, 400, "Bad Request");
        };
        let upstream = match socks5_connect(socks, &host, port, HANDSHAKE_TIMEOUT) {
            Ok(upstream) => upstream,
            Err(_) => return respond_unreachable(&mut client),
        };
        client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")?;
        client.set_read_timeout(None)?;
        splice(client, upstream);
        return Ok(());
    }

    // Plain http, which arrives as an absolute URI the origin server would not
    // understand, so the request line is rewritten on the way through.
    let Some((host, port, path)) = split_absolute_uri(&target) else {
        return respond(&mut client, 400, "Bad Request");
    };
    let mut upstream = match socks5_connect(socks, &host, port, HANDSHAKE_TIMEOUT) {
        Ok(upstream) => upstream,
        Err(_) => return respond_unreachable(&mut client),
    };

    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    for header in &headers {
        // Hop-by-hop: meaningful to this proxy, not to the origin server.
        let lowered = header.to_ascii_lowercase();
        if lowered.starts_with("proxy-connection:") || lowered.starts_with("proxy-authorization:") {
            continue;
        }
        head.push_str(header);
    }
    head.push_str("\r\n");
    upstream.write_all(head.as_bytes())?;

    // Whatever the client had already buffered is body, and belongs upstream.
    let buffered = reader.buffer().to_vec();
    if !buffered.is_empty() {
        upstream.write_all(&buffered)?;
    }
    client.set_read_timeout(None)?;
    splice(client, upstream);
    Ok(())
}

fn read_line(reader: &mut BufReader<TcpStream>, into: &mut String) -> io::Result<usize> {
    match reader.read_line(into) {
        Ok(read) => Ok(read),
        // A client that opens a connection and says nothing is not an error
        // worth reporting; it is a port scan or a warm-up probe.
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(0),
        Err(error) => Err(error),
    }
}

fn respond(client: &mut TcpStream, code: u16, reason: &str) -> io::Result<()> {
    write!(client, "HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

/// The page shown when the tunnel cannot be reached.
///
/// A bare 502 renders as the browser's own "can't reach this site", which
/// invites the reader to blame their network and start disabling things. When
/// the kill switch is holding traffic this is the *intended* outcome, so it
/// should say so in words the person can act on.
fn respond_unreachable(client: &mut TcpStream) -> io::Result<()> {
    const BODY: &str = "<!doctype html><meta charset=\"utf-8\">\
<title>Blocked by WhiteAesther</title>\
<style>body{background:#0a0a0b;color:#f2f2f3;font:16px/1.6 system-ui,sans-serif;\
display:grid;place-items:center;height:100vh;margin:0}\
main{max-width:30rem;padding:2rem}h1{font-size:1.35rem;margin:0 0 .75rem}\
p{color:1a4ad;margin:0 0 .75rem}b{color:#4ade80}</style>\
<main><h1>Traffic is blocked, not broken</h1>\
<p>WhiteAesther could not reach the tunnel, so this request was <b>held rather than \
sent in the clear</b>.</p>\
<p>The search for a working route is still running in the background, and this page \
will stop appearing as soon as one is found.</p>\
<p>To go back to an ordinary connection, open WhiteAesther and disconnect.</p></main>";
    write!(
        client,
        "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{BODY}",
        BODY.len()
    )
}

/// `host:port`, with IPv6 in brackets.
fn split_authority(target: &str, default_port: u16) -> Option<(String, u16)> {
    if let Some(rest) = target.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(port) => port.parse().ok()?,
            None => default_port,
        };
        return Some((host.to_string(), port));
    }
    match target.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_string(), port.parse().ok()?)),
        _ => Some((target.to_string(), default_port)),
    }
}

/// `http://host[:port]/path` into its parts, with the path in origin form.
fn split_absolute_uri(target: &str) -> Option<(String, u16, String)> {
    let rest = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("HTTP://"))?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = split_authority(authority, 80)?;
    Some((host, port, path.to_string()))
}

/// Opens a SOCKS5 connection to `host:port` through `socks`.
///
/// The name is passed through as a domain rather than resolved here, so DNS
/// happens inside the tunnel instead of leaking to the local resolver.
/// Opens a tunnelled connection through the SOCKS5 listener.
///
/// `timeout` bounds the connect and every read of the handshake. The bridge
/// gives it a generous value because a browser is waiting on the far side; the
/// latency probe gives it a short one, because a sample that has not landed is
/// worth less than the next sample.
pub(crate) fn socks5_connect(
    socks: SocketAddr,
    host: &str,
    port: u16,
    timeout: Duration,
) -> io::Result<TcpStream> {
    socks5_connect_with_auth(socks, host, port, timeout, None)
}

pub(crate) fn socks5_connect_with_auth(
    socks: SocketAddr,
    host: &str,
    port: u16,
    timeout: Duration,
    auth: Option<(&str, &str)>,
) -> io::Result<TcpStream> {
    let mut upstream = TcpStream::connect_timeout(&socks, timeout)?;
    upstream.set_read_timeout(Some(timeout))?;
    upstream.set_nodelay(true)?;

    if let Some((user, pass)) = auth {
        // Method 0x02: Username/Password authentication
        upstream.write_all(&[0x05, 0x01, 0x02])?;
        let mut greeting = [0_u8; 2];
        upstream.read_exact(&mut greeting)?;
        if greeting != [0x05, 0x02] {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "the SOCKS5 listener refused username/password authentication",
            ));
        }

        // Subnegotiation RFC 1929
        let u_bytes = user.as_bytes();
        let p_bytes = pass.as_bytes();
        let mut subneg = Vec::with_capacity(3 + u_bytes.len() + p_bytes.len());
        subneg.push(0x01); // version
        subneg.push(u_bytes.len() as u8);
        subneg.extend_from_slice(u_bytes);
        subneg.push(p_bytes.len() as u8);
        subneg.extend_from_slice(p_bytes);
        upstream.write_all(&subneg)?;

        let mut auth_resp = [0_u8; 2];
        upstream.read_exact(&mut auth_resp)?;
        if auth_resp[1] != 0x00 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "SOCKS5 authentication failed (invalid username or password)",
            ));
        }
    } else {
        // Greeting: SOCKS5, one method, "no authentication".
        upstream.write_all(&[0x05, 0x01, 0x00])?;
        let mut greeting = [0_u8; 2];
        upstream.read_exact(&mut greeting)?;
        if greeting != [0x05, 0x00] {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "the SOCKS5 listener refused an unauthenticated connection",
            ));
        }
    }

    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "host is too long"));
    }
    let mut request = Vec::with_capacity(host_bytes.len() + 7);
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
    request.push(host_bytes.len() as u8);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    upstream.write_all(&request)?;

    let mut reply = [0_u8; 4];
    upstream.read_exact(&mut reply)?;
    if reply[1] != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("the tunnel refused the connection (SOCKS5 reply {})", reply[1]),
        ));
    }
    // The bound address varies by type and has to be consumed before the stream
    // carries payload.
    match reply[3] {
        0x01 => drain(&mut upstream, 4 + 2)?,
        0x03 => {
            let mut length = [0_u8; 1];
            upstream.read_exact(&mut length)?;
            drain(&mut upstream, length[0] as usize + 2)?;
        }
        0x04 => drain(&mut upstream, 16 + 2)?,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown SOCKS5 address type {other}"),
            ))
        }
    }

    upstream.set_read_timeout(None)?;
    Ok(upstream)
}

fn drain(stream: &mut TcpStream, count: usize) -> io::Result<()> {
    let mut scratch = vec![0_u8; count];
    stream.read_exact(&mut scratch)
}

/// Copies in both directions until either side closes.
fn splice(client: TcpStream, upstream: TcpStream) {
    let Ok(client_reader) = client.try_clone() else { return };
    let Ok(upstream_reader) = upstream.try_clone() else { return };

    let outbound = thread::spawn(move || {
        let mut from = client_reader;
        let mut to = upstream;
        let _ = io::copy(&mut from, &mut to);
        // Half-close so the far end sees EOF rather than waiting on a request
        // that will never arrive.
        let _ = to.shutdown(Shutdown::Write);
    });

    let mut from = upstream_reader;
    let mut to = client;
    let _ = io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
    let _ = outbound.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_splits_host_and_port() {
        assert_eq!(split_authority("example.com:443", 80), Some(("example.com".into(), 443)));
        assert_eq!(split_authority("example.com", 80), Some(("example.com".into(), 80)));
        assert_eq!(split_authority("[2606:4700::1]:8443", 80), Some(("2606:4700::1".into(), 8443)));
        // Bracketed without a port still has colons in the host, which a naive
        // rsplit would take for a port separator.
        assert_eq!(split_authority("[2606:4700::1]", 443), Some(("2606:4700::1".into(), 443)));
    }

    #[test]
    fn absolute_uris_become_origin_form() {
        assert_eq!(
            split_absolute_uri("http://example.com/a/b?c=d"),
            Some(("example.com".into(), 80, "/a/b?c=d".into())),
        );
        assert_eq!(
            split_absolute_uri("http://example.com:8080/"),
            Some(("example.com".into(), 8080, "/".into())),
        );
        // No path at all still has to reach the origin as "/".
        assert_eq!(split_absolute_uri("http://example.com"), Some(("example.com".into(), 80, "/".into())));
        // https never arrives this way; it comes through CONNECT.
        assert_eq!(split_absolute_uri("https://example.com/"), None);
    }

    #[test]
    fn the_bridge_binds_and_reports_a_loopback_address() {
        let socks = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let bridge = start(socks).expect("bridge should bind");
        assert!(bridge.address().ip().is_loopback());
        assert_ne!(bridge.address().port(), 0, "port 0 must be resolved to a real one");
        bridge.stop();
    }

    #[test]
    fn a_request_for_a_dead_tunnel_is_refused_not_hung() {
        // Nothing is listening on the SOCKS port, which is what a client sees
        // if the core dies while the system proxy still points here.
        let socks = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let bridge = start(socks).expect("bridge should bind");
        let mut client = TcpStream::connect(bridge.address()).expect("bridge should accept");
        client
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 502"), "got: {response}");
        bridge.stop();
    }
}

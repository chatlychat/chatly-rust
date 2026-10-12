//! The real [`chatly::UreqTransport`], against a `std::net::TcpListener` on
//! loopback.
//!
//! `tests/contract.rs` and `tests/surface_lock.rs` inject a fake transport,
//! which is the right tool for asserting what the client *builds* — but a
//! fake cannot see what `ureq` actually puts on the wire. Three things are
//! only observable here:
//!
//! - whether our `User-Agent` survives, or whether `ureq`'s own
//!   auto-header overwrites it;
//! - whether a 404 arrives as a response with its body intact, which
//!   depends on `http_status_as_error(false)` being set; and
//! - whether a 3xx is followed, which would re-send a workspace-wide bearer
//!   token to wherever `Location` pointed.
//!
//! Still offline: loopback only, no API key, no running API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chatly::{ChatlyClient, ChatlyError, Query, RetryPolicy};

/// What the server saw.
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A canned response.
struct Canned {
    status: u16,
    reason: &'static str,
    headers: Vec<(&'static str, String)>,
    body: &'static str,
}

impl Canned {
    fn json(status: u16, reason: &'static str, body: &'static str) -> Self {
        Self { status, reason, headers: Vec::new(), body }
    }

    fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }
}

/// Serve exactly `script.len()` requests, then stop. Returns the base URL
/// and a receiver of what each request looked like.
fn serve(script: Vec<Canned>) -> (String, mpsc::Receiver<Recorded>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for canned in script {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            match handle(stream, &canned) {
                Ok(recorded) => {
                    if tx.send(recorded).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

fn handle(mut stream: TcpStream, canned: &Canned) -> std::io::Result<Recorded> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut start = String::new();
    reader.read_line(&mut start)?;
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }

    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        canned.status,
        canned.reason,
        canned.body.len()
    );
    for (name, value) in &canned.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(canned.body);
    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    Ok(Recorded {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn client(base_url: &str) -> ChatlyClient {
    ChatlyClient::builder("ck_loopback_key")
        .base_url(base_url)
        .timeout(Duration::from_secs(10))
        .retry(RetryPolicy::none())
        .build()
        .expect("valid client")
}

#[test]
fn the_real_transport_sends_our_headers_and_not_ureqs() {
    let (base, rx) = serve(vec![Canned::json(201, "Created", r#"{"id":"c_1"}"#)]);
    let out = client(&base)
        .contacts()
        .upsert(&serde_json::json!({ "externalId": "u_1" }))
        .unwrap();
    assert_eq!(out["id"], "c_1");

    let seen = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.target, "/v1/contacts");
    assert_eq!(seen.header("authorization"), Some("Bearer ck_loopback_key"));
    assert_eq!(seen.header("accept"), Some("application/json"));
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert!(seen.header("idempotency-key").is_some());
    assert_eq!(
        seen.header("user-agent"),
        Some(chatly::USER_AGENT),
        "ureq's auto User-Agent must not overwrite ours"
    );
    assert_eq!(seen.body, r#"{"externalId":"u_1"}"#);
}

#[test]
fn a_real_404_keeps_its_status_body_and_request_id() {
    // Only true because the agent is configured `http_status_as_error(false)`.
    // With ureq's default the body would be gone before we saw it.
    let (base, rx) = serve(vec![Canned::json(
        404,
        "Not Found",
        r#"{"error":{"code":"not_found","message":"no such contact"}}"#,
    )
    .with_header("x-request-id", "req_loop_404")]);

    let err = client(&base).contacts().get("c_missing").unwrap_err();
    assert!(err.is_not_found(), "{err:?}");
    assert_eq!(err.code(), Some("not_found"));
    assert_eq!(err.message(), "no such contact");
    assert_eq!(err.request_id(), Some("req_loop_404"));

    let seen = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(seen.target, "/v1/contacts/c_missing");
}

#[test]
fn a_redirect_is_not_followed_so_the_bearer_token_stays_put() {
    let (base, rx) = serve(vec![Canned::json(302, "Found", "{}")
        .with_header("location", "http://127.0.0.1:1/elsewhere")]);

    let err = client(&base).contacts().get("c_1").unwrap_err();
    assert!(matches!(err, ChatlyError::Api(_)), "{err:?}");
    assert_eq!(
        err.status(),
        Some(302),
        "a 3xx must surface, not be chased with the Authorization header attached"
    );

    let seen = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(seen.target, "/v1/contacts/c_1");
}

#[test]
fn a_repeated_query_key_survives_the_real_wire() {
    let (base, rx) = serve(vec![Canned::json(
        200,
        "OK",
        r#"{"items":[],"nextCursor":null}"#,
    )]);
    client(&base)
        .contacts()
        .list(&Query::new().filter("a:eq:1").filter("b:eq:2").limit(5))
        .unwrap();

    let seen = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        seen.target,
        "/v1/contacts?filter%5B%5D=a%3Aeq%3A1&filter%5B%5D=b%3Aeq%3A2&limit=5"
    );
    assert_eq!(
        seen.target.matches("filter%5B%5D").count(),
        2,
        "two parameters, not one comma-joined value"
    );
}

#[test]
fn a_real_429_is_retried_over_http_and_reuses_the_idempotency_key() {
    let (base, rx) = serve(vec![
        Canned::json(429, "Too Many Requests", r#"{"error":{"code":"rate_limited"}}"#)
            .with_header("retry-after", "0"),
        Canned::json(201, "Created", r#"{"id":"c_1"}"#),
    ]);
    let chatly = ChatlyClient::builder("ck_loopback_key")
        .base_url(&base)
        .timeout(Duration::from_secs(10))
        .retry(RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            respect_retry_after: true,
        })
        .build()
        .unwrap();

    let out = chatly
        .contacts()
        .upsert(&serde_json::json!({ "externalId": "u_1" }))
        .unwrap();
    assert_eq!(out["id"], "c_1");

    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        first.header("idempotency-key"),
        second.header("idempotency-key"),
        "the retry must not be a second write"
    );
}

#[test]
fn auto_pagination_works_over_real_http() {
    let (base, rx) = serve(vec![
        Canned::json(200, "OK", r#"{"items":[{"id":"a"}],"nextCursor":"cur_2"}"#),
        Canned::json(200, "OK", r#"{"items":[{"id":"b"}],"nextCursor":null}"#),
    ]);

    let ids: Vec<String> = client(&base)
        .contacts()
        .iter(&Query::new().limit(1))
        .map(|row| row.unwrap()["id"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(ids, vec!["a", "b"]);

    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(first.target, "/v1/contacts?limit=1");
    assert_eq!(second.target, "/v1/contacts?limit=1&cursor=cur_2");
}

#[test]
fn a_204_over_real_http_decodes_to_null() {
    let (base, _rx) = serve(vec![Canned::json(204, "No Content", "")]);
    let out = client(&base).macros().delete("m_1").unwrap();
    assert!(out.is_null());
}

#[test]
fn a_refused_connection_is_a_transport_error_not_a_status() {
    // Bind and drop, so the port is almost certainly free and nothing is
    // listening. Port 1 would need privileges to bind but not to fail on.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let err = client(&format!("http://127.0.0.1:{port}"))
        .contacts()
        .get("c_1")
        .unwrap_err();
    assert!(matches!(err, ChatlyError::Transport { .. }), "{err:?}");
    assert_eq!(err.status(), None);
    assert!(err.is_retryable());
    // The underlying error is reachable, so a caller can log the cause.
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn a_non_json_success_body_is_a_decode_error_carrying_what_arrived() {
    let (base, _rx) = serve(vec![Canned::json(200, "OK", "<html>nope</html>")]);
    let err = client(&base).contacts().get("c_1").unwrap_err();
    assert!(matches!(err, ChatlyError::Decode { .. }), "{err:?}");
    assert_eq!(err.body(), Some("<html>nope</html>"));
}

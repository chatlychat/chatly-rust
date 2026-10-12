//! A scripted [`Transport`] so the suite needs no network, no API key and no
//! running API — CONTRACT.md §9.
//!
//! It records every request it is handed and replays queued responses in
//! order, which is what lets a test assert on headers that were actually
//! built by the real dispatch path rather than on a reimplementation of it.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chatly::{ChatlyClient, ChatlyError, HttpRequest, HttpResponse, RetryPolicy, Transport};

/// A transport that replays a script.
#[derive(Debug, Default)]
pub struct MockTransport {
    queued: Mutex<VecDeque<Outcome>>,
    seen: Mutex<Vec<HttpRequest>>,
}

#[derive(Debug)]
enum Outcome {
    Response(HttpResponse),
    Transport(String),
}

impl MockTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Queue a JSON response.
    pub fn push_json(&self, status: u16, body: &str) -> &Self {
        self.push(HttpResponse::json(status, body))
    }

    /// Queue a response with extra headers.
    pub fn push_with_headers(&self, status: u16, body: &str, headers: &[(&str, &str)]) -> &Self {
        let mut all = vec![("content-type".to_owned(), "application/json".to_owned())];
        all.extend(
            headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        self.push(HttpResponse::new(status, all, body.as_bytes().to_vec()))
    }

    /// Queue a fully built response.
    pub fn push(&self, response: HttpResponse) -> &Self {
        if let Ok(mut queue) = self.queued.lock() {
            queue.push_back(Outcome::Response(response));
        }
        self
    }

    /// Queue a failure to reach the server at all.
    pub fn push_transport_error(&self, message: &str) -> &Self {
        if let Ok(mut queue) = self.queued.lock() {
            queue.push_back(Outcome::Transport(message.to_owned()));
        }
        self
    }

    /// Every request handed to this transport, in order.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// The single request this transport saw. Panics if it saw a different
    /// number, which is itself the assertion most of these tests want.
    pub fn only_request(&self) -> HttpRequest {
        let requests = self.requests();
        assert_eq!(requests.len(), 1, "expected exactly one request");
        requests.into_iter().next().expect("checked length")
    }

    pub fn request_count(&self) -> usize {
        self.requests().len()
    }
}

impl Transport for MockTransport {
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, ChatlyError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request.clone());
        }
        let next = self.queued.lock().ok().and_then(|mut q| q.pop_front());
        match next {
            Some(Outcome::Response(response)) => Ok(response),
            Some(Outcome::Transport(message)) => Err(ChatlyError::transport(message)),
            None => panic!(
                "MockTransport ran out of scripted responses at request {} ({} {})",
                self.request_count(),
                request.method,
                request.url
            ),
        }
    }
}

/// A client on the **production default base URL**, talking to `mock`.
///
/// Deliberately does not call `.base_url(...)`. CONTRACT.md §1 records that
/// four SDKs shipped a dev host as their default and no test caught it
/// "because every test passes an explicit URL" — so the tests here assert
/// the URL the client actually built instead.
pub fn client(mock: &Arc<MockTransport>) -> ChatlyClient {
    ChatlyClient::builder("ck_test_key")
        .shared_transport(mock.clone())
        .retry(no_retries())
        .build()
        .expect("valid client")
}

/// A client that retries with no sleeping, so retry tests stay instant.
pub fn retrying_client(mock: &Arc<MockTransport>, max_attempts: u32) -> ChatlyClient {
    ChatlyClient::builder("ck_test_key")
        .shared_transport(mock.clone())
        .retry(RetryPolicy {
            max_attempts,
            initial_backoff: std::time::Duration::ZERO,
            max_backoff: std::time::Duration::ZERO,
            respect_retry_after: false,
        })
        .build()
        .expect("valid client")
}

pub fn no_retries() -> RetryPolicy {
    RetryPolicy::none()
}

//! Visitors — opening a conversation with someone who has not written yet.
//!
//! ## One method, and why the sibling reads are absent
//!
//! `GET /v1/visitors/live` and `GET /v1/visitors/{contactId}/pages` sit on the
//! same controller and carry **no `@AllowApiKey()`**, so a `ck_` key gets a
//! 403 from both. They are not surface. Shipping a method that always fails
//! is worse than not shipping it: the failure arrives at runtime, in
//! production, in someone else's integration — which is the rule the parent
//! module states for `reactions` and for reading a channel's secret.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `visitors` namespace. Get one from [`ChatlyClient::visitors`].
#[derive(Debug, Clone, Copy)]
pub struct Visitors<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Visitors<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `POST /v1/visitors/{contactId}/start-conversation` — open a thread
    /// with a contact who did **not** write first.
    ///
    /// Takes no body and no query parameters: the contact id is the whole
    /// input. Answers a single object, `{ "conversationId": … }`.
    ///
    /// ## A 403 here is probably not your scopes
    ///
    /// This route is gated by a per-workspace opt-in that is **off by
    /// default**: `settings.proactiveOutreach.apiKeysMayStartConversations`.
    /// An owner or admin turns it on with `PATCH /v1/workspace/settings`,
    /// which needs a signed-in person and so is not reachable from this
    /// crate; the current value is readable with
    /// [`Workspace::settings`](crate::resources::Workspace::settings), which
    /// is.
    ///
    /// The scope is deliberately not the opt-in. `conversations:write` was
    /// already grantable and is already held by keys minted for ticket bulk
    /// actions and schedule-send, so opening this route on the permission
    /// alone would have handed cold outreach to credentials nobody
    /// re-reviewed. Check the setting before you go looking at your key.
    ///
    /// ## Rate limit, and what an absent visitor gets
    ///
    /// **Three per contact per hour**, counted in Redis per workspace and per
    /// *contact* rather than per caller — the abuse shape is N threads at one
    /// person, which a per-caller limit cannot see.
    ///
    /// This route can answer **two different 429s and they do not look the
    /// same**, which matters because only one of them tells you how long to
    /// wait:
    ///
    /// - The **per-contact limit** throws `RateLimitedError`, whose TTL goes
    ///   into the JSON body at `error.details.retryAfterSeconds`. Nothing
    ///   sets a `Retry-After` header for it — the exception filter sets no
    ///   headers, and the only thing that synthesises a plain `Retry-After`
    ///   is a hook deriving it from the `Retry-After-<bucket>` headers
    ///   `@nestjs/throttler` emits, which this refusal does not produce. So
    ///   [`ChatlyError::retry_after`] is **`None`** here, and the TTL is
    ///   readable only by parsing [`ApiError::body`](crate::ApiError::body).
    /// - The **global throttler buckets** this route inherits (`short`,
    ///   30/second; `login`, 100/15 minutes) do emit those bucket headers, so
    ///   a 429 from one of them carries a `Retry-After` and `retry_after` is
    ///   populated.
    ///
    /// A caller that backs off only when `retry_after` is `Some` therefore
    /// retries the per-contact limit immediately. Treat `None` on this route
    /// as "wait out the hour", not as "no delay required".
    ///
    /// ### This is one of the few places the SDKs genuinely differ
    ///
    /// The missing header is a property of the **route**; whether a client
    /// surfaces the wait anyway is a property of **that client's error
    /// construction**, and the eight do not agree. This crate builds
    /// `retry_after` from the header alone — `parse_retry_after` in
    /// `src/retry.rs` takes an `Option<&str>` header and nothing else, and
    /// neither `src/error.rs` nor `src/client.rs` mentions `details` — so
    /// here it is `None` and you must parse
    /// [`ApiError::body`](crate::ApiError::body) yourself:
    ///
    /// ```no_run
    /// # use chatly::{ChatlyClient, ChatlyError};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let chatly = ChatlyClient::new("ck_x")?;
    /// match chatly.visitors().start_conversation("cont_1") {
    ///     Err(ChatlyError::RateLimited { error, retry_after }) => {
    ///         // `retry_after` is None on the per-contact limit.
    ///         let wait = retry_after.map(|d| d.as_secs()).or_else(|| {
    ///             serde_json::from_str::<serde_json::Value>(&error.body)
    ///                 .ok()?["error"]["details"]["retryAfterSeconds"]
    ///                 .as_u64()
    ///         });
    ///         eprintln!("rate limited; wait {wait:?} seconds");
    ///     }
    ///     other => {
    ///         other?;
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The Ruby and Python clients *do* fall back to the body
    /// (`lib/chatly/errors.rb`'s `retry_after_from`, and the `status == 429`
    /// arm of `src/chatly/errors.py`, both preferring the header when both
    /// are present), so their `retry_after` is populated on this same
    /// refusal. Node, Go and PHP read headers only, like this crate.
    ///
    /// So CONTRACT.md §10.4 — "a 429 the rate-limit type with `retry_after`
    /// populated" — holds for the throttler's 429 everywhere, and on *this*
    /// route's own limit it is language-dependent. Do not port a retry loop
    /// from the Python or Ruby examples here and assume the wait arrives.
    ///
    /// **An offline visitor is not notified at all**: the greeting is
    /// persisted and the widget renders it from history on their next page
    /// view. No email, no push, no provider message, and nothing queued or
    /// dropped. An integration that needs to reach someone who has left the
    /// site wants an email or SMS conversation, not this.
    ///
    /// ## Idempotency — the opposite case to `channels().rotate_secret`
    ///
    /// The controller declares `@RequireIdempotencyKey()` at class level with
    /// **no opt-out on this handler**, so the key is *required*: without it
    /// the API answers 428 in production. This crate sends one per call, so a
    /// retried timeout is a **replay of the same thread** rather than a
    /// second one — unlike `rotate_secret`, which declares `@NoIdempotency`
    /// and mints a second secret on a blind retry.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Permission`] when the workspace
    /// opt-in is off (read the message, not just the status — it names the
    /// setting), [`ChatlyError::RateLimited`] past three in an hour for this
    /// contact (with `retry_after: None` — see above), and
    /// [`ChatlyError::NotFound`] if no such contact.
    pub fn start_conversation(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/visitors/{}/start-conversation",
                encode_path_segment(id)
            ),
            &Query::new(),
            None,
        )
    }
}

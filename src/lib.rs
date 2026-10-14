//! The official Rust SDK for [Chatly](https://chatlychat.com) — live chat,
//! shared inbox, help centre and AI agents.
//!
//! ```no_run
//! use chatly::{ChatlyClient, ContactInput, Query};
//!
//! fn main() -> Result<(), chatly::ChatlyError> {
//!     let chatly = ChatlyClient::new(std::env::var("CHATLY_API_KEY").unwrap_or_default())?;
//!
//!     // Always worth calling first: it turns a mis-scoped key into an
//!     // obvious error now rather than a 403 from somewhere else later.
//!     println!("{}", chatly.whoami()?);
//!
//!     chatly
//!         .contacts()
//!         .upsert(&ContactInput::new().external_id("u_42").email("ada@example.com"))?;
//!
//!     for contact in chatly.contacts().iter(&Query::new().limit(100)) {
//!         println!("{}", contact?);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Server-side only
//!
//! This crate authenticates with a workspace API key (`ck_…`). That key is a
//! bearer credential for the **entire workspace**: every contact, every
//! conversation, every report. It belongs in server-side code and in a
//! secret store, never in a browser, a mobile binary or a public repository.
//! The browser SDK is `@chatlychat/sdk-js`.
//!
//! # What this crate does for you
//!
//! - **Auth and headers.** `Authorization: Bearer`, `Accept`,
//!   `Content-Type`, and a `User-Agent` of [`USER_AGENT`].
//! - **Idempotency.** Every `POST`/`PUT`/`PATCH`/`DELETE` carries a fresh
//!   `Idempotency-Key`, so an SDK-level retry cannot double-write. Pin one
//!   across calls with [`ChatlyClient::with_idempotency_key`].
//! - **Retries.** Connection failures, 429 and 5xx only — three attempts,
//!   exponential backoff with jitter, `Retry-After` honoured, and the same
//!   `Idempotency-Key` on every attempt. A 4xx that is not 429 is never
//!   retried. See [`RetryPolicy`].
//! - **Typed errors.** [`ChatlyError`], with a variant per case a caller
//!   branches on, carrying `status`, `code`, `message`, `request_id` and the
//!   raw `body`.
//! - **Auto-pagination.** [`PageIter`] walks cursors for you and cannot spin
//!   forever on a repeated cursor.
//! - **Webhook verification** ([`verify_webhook_signature`]) and **widget
//!   identity hashing** ([`identity_hash`]), neither of which needs a
//!   client.
//! - **An escape hatch.** [`ChatlyClient::request`] reaches any
//!   key-accessible route through the same machinery.
//!
//! # What it deliberately does not do
//!
//! **It does not model response bodies.** Every method returns
//! [`serde_json::Value`] (or a [`Page`] of them). That is not laziness: this
//! crate is written against a hand-maintained method list
//! (`packages/sdk-spec/surface.json`) that pins verbs, paths and the
//! TOP-LEVEL shape of each answer — array, page, object or 204 — and
//! nothing at all about the fields inside. A hand-written `struct Contact`
//! would be a guess that compiles, goes stale silently, and makes the test
//! suite agree with it. The top-level shape is worth knowing and this crate
//! does tell you, per method; see also the shape table in
//! `tests/common/shapes.rs`, which is the hand-written second opinion the
//! parity gate diffs against the manifest. If you want field-level types,
//! write them yourself and use
//! [`ChatlyClient::request_as`] or [`ChatlyClient::paginate`], both of which
//! are generic over any `Deserialize`.
//!
//! It is also **blocking**. There is no async variant and no feature flag
//! for one: a blocking client needs no runtime, and bolting `async` on with
//! a thread pool would be worse than the `tokio::task::spawn_blocking` call
//! you can write yourself.
//!
//! # Dependencies
//!
//! Three: [`ureq`] for HTTP and TLS, `serde` and `serde_json`. CONTRACT.md
//! §9 permits Rust exactly that much. SHA-256, HMAC, hex, UUID v4 and
//! percent-encoding are implemented here on `std` rather than pulled in, so
//! you do not inherit `sha2`, `hmac`, `uuid`, `rand`, `url` and
//! `percent-encoding` in order to send a message.
//!
//! # Where the behaviour is specified
//!
//! `packages/sdk-spec/CONTRACT.md` in the Chatly monorepo is the binding
//! contract every Chatly backend SDK implements, and the source of the
//! pinned test vectors this crate is checked against. Doc comments here
//! cite it by section.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
#![warn(clippy::all)]

mod client;
mod crypto;
mod decode;
mod error;
mod identity;
mod namespaces;
mod pagination;
mod query;
pub mod resources;
mod retry;
mod transport;
mod webhooks;

pub use client::{
    ChatlyClient, ChatlyClientBuilder, DEFAULT_BASE_URL, DEFAULT_TIMEOUT, USER_AGENT,
};
pub use error::{ApiError, ChatlyError};
pub use identity::identity_hash;
pub use pagination::{Page, PageIter};
pub use query::Query;
pub use retry::RetryPolicy;
pub use transport::{HttpRequest, HttpResponse, Transport, UreqTransport};
pub use webhooks::{
    verify_webhook_signature, verify_webhook_signature_with, VerifyOptions,
    WEBHOOK_SIGNATURE_HEADER, WEBHOOK_SIGNATURE_TOLERANCE_SECONDS,
};

pub use resources::{
    Ai, Billing, BotRoutes, Bots, Brands, BulkContactsInput, CanvasSteps, Channels, ContactInput,
    Contacts, Conversations, CustomerCardEndpoints, CustomerCards, DocsSources, EmailTemplates,
    EventInput, Events, Kb, KnowledgeGaps, Macros, MessageInput, MessageKind, Reports, Search,
    Tickets, Triggers, Webhooks, Workspace, AGENT_HANDLING_SETTABLE_STATES, AI_PROVIDERS,
    ATTRIBUTION_DIMENSIONS, CHANNEL_TYPES, CUSTOMER_CARD_CAP, DELIVERY_STATUSES, DOCS_PROVIDERS,
    EMAIL_TEMPLATE_KINDS, GAP_STATUSES, MACRO_ACTION_KEYS, MACRO_LIBRARY_CAP, MACRO_SCOPES,
    ROUTE_CONDITION_OPS, SEARCH_KINDS, TICKET_BULK_ACTIONS, TRIGGER_CONDITION_OPS,
    USAGE_EVENT_TYPES,
};

/// Re-exported so you can build request bodies with `serde_json::json!`
/// without adding `serde_json` to your own `Cargo.toml`, and so the version
/// of [`serde_json::Value`] you match on is the one this crate returns.
pub use serde_json;

/// This crate's version, as it appears in the `User-Agent`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

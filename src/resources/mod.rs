//! One module per namespace in `packages/sdk-spec/surface.json`.
//!
//! Each namespace is a zero-cost borrowed view over a [`crate::ChatlyClient`]
//! — `client.contacts()` hands back a `Contacts<'_>` that holds a reference
//! and nothing else, so reaching for one in a loop allocates nothing.
//!
//! ## What is deliberately absent
//!
//! Only routes carrying `@AllowApiKey()` in `apps/api` are SDK surface; the
//! rest answer a `ck_` key with 403 because they need a signed-in person. So
//! there is no `reactions` namespace (a reaction records *who* reacted, and
//! a key is not a person), no contact block/unblock (same reason — the row
//! records who blocked), and no `GET /v1/channels/{id}/secret` (read
//! silently versus rotate loudly is the line there, not read versus write).
//! Shipping a method that always fails is worse than not shipping it: the
//! failure arrives at runtime, in production, in someone else's
//! integration.
//!
//! Two methods here come close to that line and are kept, with the reason in
//! their own docs: [`Conversations::unread_counts`] answers per-USER state
//! that is always empty for a key, and
//! [`Macros::template`] can read the starter catalogue while installing from
//! it is closed.
//!
//! Anything key-accessible without a typed method here is reachable through
//! [`crate::ChatlyClient::request`].

mod ai;
mod billing;
mod bot_routes;
mod bots;
mod brands;
mod canvas_steps;
mod channels;
mod contacts;
mod conversations;
mod conversations_ext;
mod customer_card_endpoints;
mod customer_cards;
mod docs_sources;
mod email_templates;
mod events;
mod kb;
mod knowledge_gaps;
mod macros;
mod reports;
mod search;
mod tickets;
mod triggers;
mod visitors;
mod webhooks;
mod workspace;

pub use ai::{Ai, AI_PROVIDERS};
pub use billing::{Billing, USAGE_EVENT_TYPES};
pub use bot_routes::{BotRoutes, ROUTE_CONDITION_OPS};
pub use bots::{Bots, DELIVERY_STATUSES};
pub use brands::Brands;
pub use canvas_steps::CanvasSteps;
pub use channels::{Channels, CHANNEL_TYPES};
pub use contacts::{BulkContactsInput, ContactInput, Contacts};
pub use conversations::{Conversations, MessageInput, MessageKind, AGENT_HANDLING_SETTABLE_STATES};
pub use customer_card_endpoints::CustomerCardEndpoints;
pub use customer_cards::{CustomerCards, CUSTOMER_CARD_CAP};
pub use docs_sources::{DocsSources, DOCS_PROVIDERS};
pub use email_templates::{EmailTemplates, EMAIL_TEMPLATE_KINDS};
pub use events::{EventInput, Events};
pub use kb::Kb;
pub use knowledge_gaps::{KnowledgeGaps, GAP_STATUSES};
pub use macros::{Macros, MACRO_ACTION_KEYS, MACRO_LIBRARY_CAP, MACRO_SCOPES};
pub use reports::{Reports, ATTRIBUTION_DIMENSIONS};
pub use search::{Search, SEARCH_KINDS};
pub use tickets::{Tickets, TICKET_BULK_ACTIONS};
pub use triggers::{Triggers, TRIGGER_CONDITION_OPS};
pub use visitors::Visitors;
pub use webhooks::Webhooks;
pub use workspace::Workspace;

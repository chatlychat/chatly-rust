//! One module per namespace in `packages/sdk-spec/surface.json`.
//!
//! Each namespace is a zero-cost borrowed view over a [`crate::ChatlyClient`]
//! — `client.contacts()` hands back a `Contacts<'_>` that holds a reference
//! and nothing else, so reaching for one in a loop allocates nothing.
//!
//! ## What is deliberately absent
//!
//! Only the 140 routes carrying `@AllowApiKey()` in `apps/api` are SDK
//! surface; the other 746 answer a `ck_` key with 403 because they need a
//! signed-in person. So there is no `reactions` namespace (a reaction
//! records *who* reacted, and a key is not a person), no channel
//! configuration (it needs a `channels:write` scope no key is granted), and
//! no endpoint management under [`Webhooks`]. Shipping a method that always
//! fails is worse than not shipping it: the failure arrives at runtime, in
//! production, in someone else's integration.
//!
//! Anything key-accessible without a typed method here is reachable through
//! [`crate::ChatlyClient::request`].

mod billing;
mod channels;
mod contacts;
mod conversations;
mod events;
mod kb;
mod macros;
mod reports;
mod search;
mod tickets;
mod webhooks;
mod workspace;

pub use billing::Billing;
pub use channels::Channels;
pub use contacts::{BulkContactsInput, ContactInput, Contacts};
pub use conversations::{Conversations, MessageInput, MessageKind, AGENT_HANDLING_SETTABLE_STATES};
pub use events::{EventInput, Events};
pub use kb::Kb;
pub use macros::{Macros, MACRO_ACTION_KEYS, MACRO_SCOPES};
pub use reports::Reports;
pub use search::{Search, SEARCH_KINDS};
pub use tickets::{Tickets, TICKET_BULK_ACTIONS};
pub use webhooks::Webhooks;
pub use workspace::Workspace;

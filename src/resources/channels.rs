//! Channels — the mediums a workspace can be reached on.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `type` values [`Channels::create`] accepts.
///
/// Seventeen, as the API's enum spells them. `api` is the custom channel —
/// the only one with a Chatly-issued secret, and so the only one
/// [`Channels::rotate_secret`] works on. `telegram_user` is deliberately
/// absent: it needs an MTProto session this route cannot create, so offering
/// it would only allow a channel that can never send or receive.
pub const CHANNEL_TYPES: [&str; 17] = [
    "web",
    "email",
    "whatsapp",
    "messenger",
    "instagram",
    "sms",
    "voice",
    "api",
    "telegram",
    "discord",
    "apple",
    "line",
    "viber",
    "slack",
    "msteams",
    "tiktok",
    "twitter",
];

/// The `channels` namespace. Get one from [`ChatlyClient::channels`].
///
/// Note what is still absent: there is no `delete` here, and no way to read a
/// channel's secret back. `GET /v1/channels/{id}/secret` is the one channel
/// route that is deliberately **not** key-accessible, because that secret
/// authenticates inbound messages carrying an arbitrary `contactId` — holding
/// it means being able to impersonate any customer in the workspace. Rotate
/// it instead.
#[derive(Debug, Clone, Copy)]
pub struct Channels<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Channels<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/channels` — every channel in the workspace, as a **JSON
    /// array**.
    ///
    /// Pagination-exempt: a settings list, returned whole. No envelope, no
    /// cursor, no iterator.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/channels", query)
    }

    /// `GET /v1/channels/{id}` — one channel.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such channel.
    pub fn get(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/channels/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `GET /v1/channels/adapters` — the channel types this deployment can
    /// actually serve, as a **bare array of strings**.
    ///
    /// An array of plain strings, not objects — the one route on this
    /// surface shaped like that. Sorted and de-duplicated.
    ///
    /// It is the union of the inbound-adapter registry **and** the channels
    /// delivered over realtime, which matters because `web` has no inbound
    /// adapter by design — the widget arrives over the socket and
    /// `/v1/widget`, not a provider webhook — so a list built from the
    /// registry alone reported the flagship channel as unsupported.
    ///
    /// Use it to decide what is connectable; [`CHANNEL_TYPES`] is what the
    /// *schema* accepts, which is a longer list.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn adapters(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/channels/adapters", query)
    }

    /// `POST /v1/channels` — create a channel.
    ///
    /// `type` (one of [`CHANNEL_TYPES`]), `name` (1–120) and `publicId`
    /// (1–64) are required; `enabled` defaults to true, `config` to `{}`,
    /// and `brandId` to **the workspace's default brand** — which is a
    /// `NotFound` for "Default brand", not a null, if the workspace has no
    /// default. That 404 is about the brand and not about the channel.
    ///
    /// `config` is normalised per type *before* the transaction opens, so an
    /// unsupported carrier is a 400 rather than a rolled-back write. Counts
    /// against the plan's channel limit.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a bad `config`,
    /// [`ChatlyError::NotFound`] for a workspace with no default brand.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/channels", &Query::new(), Some(body))
    }

    /// `PATCH /v1/channels/{id}` — update a channel.
    ///
    /// `name`, `enabled`, `autoCreateTickets`, `autoAcknowledge`,
    /// `acknowledgeTemplate`, `acknowledgeSubject`, `config` and
    /// `configMode`. **`type` and `publicId` cannot be changed** — external
    /// systems depend on them — and they are simply not in the schema.
    ///
    /// Three things worth knowing:
    ///
    /// - **`config` merges by default** (RFC 7396): the keys you send are
    ///   written, the rest are left alone, and an explicit `null` deletes a
    ///   key. `configMode: "replace"` is the old overwrite-everything
    ///   behaviour, kept for a caller who means it and made something you
    ///   have to say rather than something that happens when you forget to
    ///   spread.
    /// - **An empty patch is a 409**, not a no-op.
    /// - `acknowledgeTemplate: null` restores the built-in copy, where
    ///   omitting it changes nothing. The two are different requests.
    ///
    /// Secrets inside `config` are returned redacted, and a patch that sends
    /// a redacted value back is understood as "leave it alone" rather than
    /// storing the mask. A changed identity-verification secret keeps the old
    /// one verifying for a grace window.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Conflict`] for an empty patch,
    /// [`ChatlyError::NotFound`] if no such channel.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/channels/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/channels/{id}/secret/rotate` — issue a new `csk_` secret
    /// for a custom (`api`) channel.
    ///
    /// **Only `type: "api"` has a Chatly-issued secret**; every other
    /// channel's credential is issued by its provider, so this answers a
    /// **409** for them rather than pretending to have secured something.
    ///
    /// A **hard cut with no grace window**, deliberately: a second valid
    /// secret would keep a leaked one working for the overlap, which defeats
    /// rotating. The integration is expected to break until the new value is
    /// deployed.
    ///
    /// Takes no body, and is the one write on this controller that declares
    /// `@NoIdempotency` — the SDK still sends a key, which the server
    /// ignores. So an SDK-level retry of a timeout mints a *second* secret
    /// and invalidates the first: treat a timeout here as "state unknown"
    /// and re-read, rather than retrying blind.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Conflict`] for a non-`api`
    /// channel, [`ChatlyError::NotFound`] if no such channel.
    pub fn rotate_secret(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!("/v1/channels/{}/secret/rotate", encode_path_segment(id)),
            &Query::new(),
            None,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_types_match_the_apis_enum() {
        assert_eq!(CHANNEL_TYPES.len(), 17);
        // The flagship, and the one with no inbound adapter.
        assert!(CHANNEL_TYPES.contains(&"web"));
        // The only type with a Chatly-issued secret.
        assert!(CHANNEL_TYPES.contains(&"api"));
        // Needs an MTProto session this route cannot create.
        assert!(!CHANNEL_TYPES.contains(&"telegram_user"));
    }
}

//! Channels — the mediums a workspace can be reached on.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `channels` namespace. Get one from [`ChatlyClient::channels`].
///
/// Read-only here by design. Channel *configuration* needs a
/// `channels:write` scope that no API key is granted, so a `configure`
/// method would return 403 to every caller — and CONTRACT.md is explicit
/// that shipping a method which always fails is worse than not shipping it.
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
}

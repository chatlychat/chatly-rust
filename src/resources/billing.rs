//! Billing — the plan catalogue and this workspace's usage.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::Query;

/// The `billing` namespace. Get one from [`ChatlyClient::billing`].
///
/// Read-only. Changing a subscription is not key-accessible: it needs a
/// signed-in person with billing permission, so there is deliberately no
/// `subscribe` or `cancel` here.
#[derive(Debug, Clone, Copy)]
pub struct Billing<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Billing<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/billing/plans` — the plan catalogue.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn plans(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/billing/plans", query)
    }

    /// `GET /v1/billing/usage` — this workspace's metered usage.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn usage(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/billing/usage", query)
    }
}

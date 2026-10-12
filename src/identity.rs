//! Widget identity verification — the backend half.
//!
//! When a workspace turns identity verification on, the browser widget must
//! prove that the `externalId` it claims was issued by your backend rather
//! than typed into a developer console. Without it, anyone can open a chat
//! as anyone else's account and read that account's history.
//!
//! Your server computes [`identity_hash`] and renders the digest into the
//! page; the widget presents it alongside the id:
//!
//! ```text
//! Chatly('identify', { externalId: user.id, userHash, email: user.email })
//! ```
//!
//! The secret is the workspace's identity-verification secret and must never
//! reach a browser — only the digest does. It is not the webhook signing
//! secret and not the API key; all three are separate values.

use crate::crypto;
use crate::error::ChatlyError;

/// Compute the `userHash` the widget needs, as lowercase hex.
///
/// HMAC-SHA256 of `external_id` under `secret`.
///
/// # Errors
///
/// [`ChatlyError::Config`] if either argument is empty. An empty secret
/// would produce a digest an attacker can also produce, and an empty id
/// would authenticate a session as nobody — both are silent authentication
/// bypasses, so they are refused rather than hashed.
///
/// ```
/// # fn main() -> Result<(), chatly::ChatlyError> {
/// let hash = chatly::identity_hash("sek_test_secret", "user_42")?;
/// assert_eq!(
///     hash,
///     "5e26772dce97d5ba995796e621f83f2e97e6f8c3f9386dbf153a98ab24c10271"
/// );
/// # Ok(()) }
/// ```
pub fn identity_hash(secret: &str, external_id: &str) -> Result<String, ChatlyError> {
    if secret.is_empty() {
        return Err(ChatlyError::Config(
            "identity_hash: secret is empty".to_owned(),
        ));
    }
    if external_id.is_empty() {
        return Err(ChatlyError::Config(
            "identity_hash: external_id is empty".to_owned(),
        ));
    }
    let digest = crypto::hmac_sha256(secret.as_bytes(), &[external_id.as_bytes()]);
    Ok(crypto::hex_encode(&digest))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// CONTRACT.md §10.8, verbatim. Cross-checked against
    /// `printf 'user_42' | openssl dgst -sha256 -hmac sek_test_secret`.
    /// "This vector is computed and cross-checked in two implementations.
    /// If yours disagrees, yours is wrong."
    #[test]
    fn matches_the_pinned_cross_language_vector() {
        assert_eq!(
            identity_hash("sek_test_secret", "user_42").unwrap(),
            "5e26772dce97d5ba995796e621f83f2e97e6f8c3f9386dbf153a98ab24c10271"
        );
    }

    #[test]
    fn rejects_an_empty_secret_or_id() {
        assert!(matches!(
            identity_hash("", "user_42"),
            Err(ChatlyError::Config(_))
        ));
        assert!(matches!(
            identity_hash("sek_test_secret", ""),
            Err(ChatlyError::Config(_))
        ));
        assert!(matches!(identity_hash("", ""), Err(ChatlyError::Config(_))));
    }

    #[test]
    fn the_output_is_lowercase_hex_of_the_right_length() {
        let hash = identity_hash("sek_test_secret", "user_42").unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn the_secret_and_the_id_are_not_interchangeable() {
        // Guards against an argument-order slip in a port of this code:
        // HMAC is not symmetric in key and message.
        assert_ne!(
            identity_hash("sek_test_secret", "user_42").unwrap(),
            identity_hash("user_42", "sek_test_secret").unwrap()
        );
    }

    #[test]
    fn distinct_ids_and_secrets_give_distinct_digests() {
        let a = identity_hash("sek_test_secret", "user_42").unwrap();
        assert_ne!(a, identity_hash("sek_test_secret", "user_43").unwrap());
        assert_ne!(a, identity_hash("sek_other_secret", "user_42").unwrap());
    }

    #[test]
    fn handles_non_ascii_ids() {
        // External ids are customer-supplied; a UTF-8 id must hash its bytes,
        // not panic and not be truncated.
        let hash = identity_hash("sek_test_secret", "användare_42").unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn a_long_secret_is_hashed_down_to_the_block_size() {
        // Exercises the RFC 2104 key > 64 bytes branch through the public API.
        let hash = identity_hash(&"s".repeat(200), "user_42").unwrap();
        assert_eq!(hash.len(), 64);
    }
}

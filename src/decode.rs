//! Turning a success response into JSON.
//!
//! Split out of `client.rs` for the contract's 650-line ceiling (§9), and it
//! is a real seam: everything here is about interpreting a 2xx body, and
//! nothing about building a request or deciding whether to retry one.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::ChatlyError;
use crate::transport::HttpResponse;

/// Decode a success response body into JSON.
///
/// A 204, or any empty body, decodes to [`Value::Null`] rather than an error:
/// several write routes answer with no content and a caller should not have
/// to special-case that.
pub(crate) fn decode_response(response: &HttpResponse) -> Result<Value, ChatlyError> {
    if response.status == 204 || response.body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&response.body).map_err(|err| ChatlyError::Decode {
        message: format!("response was not valid JSON: {err}"),
        body: response.body_text(),
    })
}

pub(crate) fn decode_value<T: DeserializeOwned>(value: Value) -> Result<T, ChatlyError> {
    let rendered = value.to_string();
    serde_json::from_value(value).map_err(|err| ChatlyError::Decode {
        message: format!("response did not match the expected shape: {err}"),
        body: rendered,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::pagination::Page;

    #[test]
    fn empty_and_204_bodies_decode_to_null() {
        assert_eq!(
            decode_response(&HttpResponse::new(204, vec![], Vec::new())).unwrap(),
            Value::Null
        );
        assert_eq!(
            decode_response(&HttpResponse::new(200, vec![], b"  \n ".to_vec())).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn a_non_json_success_body_is_a_decode_error_carrying_the_body() {
        let err = decode_response(&HttpResponse::new(200, vec![], b"<html/>".to_vec()))
            .expect_err("should not decode");
        assert!(matches!(err, ChatlyError::Decode { .. }));
        assert_eq!(err.body(), Some("<html/>"));
    }

    #[test]
    fn decode_value_reports_a_shape_mismatch_with_the_body() {
        let err = decode_value::<Page<Value>>(serde_json::json!({ "items": 7 }))
            .expect_err("items is not an array");
        assert!(matches!(err, ChatlyError::Decode { .. }));
        assert!(err.body().unwrap_or_default().contains("7"));
    }
}

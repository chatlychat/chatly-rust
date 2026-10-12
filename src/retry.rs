//! Retry policy, and the two small rules the dispatcher needs from it.
//!
//! CONTRACT.md §4: retry only connection errors, 429 and 5xx; never a 4xx
//! that is not 429; exponential backoff with jitter, 3 attempts by default;
//! honour `Retry-After`; and a retried write reuses the `Idempotency-Key` of
//! the attempt that failed. The last of those is enforced in
//! [`crate::ChatlyClient`], which mints the key before the retry loop; the
//! rest is here.

use std::time::Duration;

use crate::crypto;

/// How this client retries.
///
/// Which errors are even eligible is not configurable: see
/// [`crate::ChatlyError::is_retryable`]. A 409 in particular is never
/// retried, because it is how the API reports an `Idempotency-Key` reused
/// with a different body — retrying would report the same conflict again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `3` by default; `1` disables
    /// retries.
    pub max_attempts: u32,
    /// Backoff before the second attempt. Doubles each time.
    pub initial_backoff: Duration,
    /// Ceiling on any single backoff, including one derived from
    /// `Retry-After`.
    pub max_backoff: Duration,
    /// Whether a numeric `Retry-After` header overrides the computed
    /// backoff.
    pub respect_retry_after: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(20),
            respect_retry_after: true,
        }
    }
}

impl RetryPolicy {
    /// Never retry anything: one attempt, then the error.
    pub fn none() -> Self {
        Self { max_attempts: 1, ..Self::default() }
    }

    /// A policy with `max_attempts` attempts and the default backoff.
    pub fn attempts(max_attempts: u32) -> Self {
        Self { max_attempts, ..Self::default() }
    }

    /// How long to wait before attempt `attempt + 1`.
    ///
    /// Exponential, jittered into `[0.5x, 1.0x)` of the computed delay so a
    /// fleet of workers that all hit the same 429 do not all come back on
    /// the same millisecond. A numeric `Retry-After` wins when present and
    /// `respect_retry_after` is set, and is itself capped by `max_backoff`
    /// so a server cannot park a thread for an hour.
    pub(crate) fn backoff_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if self.respect_retry_after {
            if let Some(retry_after) = retry_after {
                return retry_after.min(self.max_backoff);
            }
        }
        let shift = attempt.saturating_sub(1).min(16);
        let nanos = u64::try_from(self.initial_backoff.as_nanos())
            .unwrap_or(u64::MAX)
            .saturating_mul(1u64 << shift);
        let capped = nanos.min(u64::try_from(self.max_backoff.as_nanos()).unwrap_or(u64::MAX));
        if capped == 0 {
            return Duration::ZERO;
        }
        let half = capped / 2;
        let jitter = if half == 0 { 0 } else { crypto::random_u64() % half };
        Duration::from_nanos(half.saturating_add(jitter))
    }
}

/// `POST`, `PUT`, `PATCH` and `DELETE` are writes and carry an
/// `Idempotency-Key` (CONTRACT.md §2).
///
/// `DELETE` has no body but still gets one: `DELETE /v1/macros/{id}` retried
/// is only safe if the server can tell the retry from a second delete.
pub(crate) fn is_write(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

/// Parse `Retry-After` as a count of seconds.
///
/// HTTP also allows an HTTP-date there; this crate does not parse dates and
/// returns `None` for one, falling back to exponential backoff rather than
/// guessing. Our own 429s send seconds.
pub(crate) fn parse_retry_after(header: Option<&str>) -> Option<Duration> {
    let seconds: u64 = header?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_exactly_post_put_patch_delete() {
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert!(is_write(method), "{method}");
        }
        for method in ["GET", "HEAD", "OPTIONS", "TRACE"] {
            assert!(!is_write(method), "{method}");
        }
    }

    #[test]
    fn retry_after_parses_only_integer_seconds() {
        assert_eq!(parse_retry_after(Some("7")), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(Some(" 7 ")), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(Some("0")), Some(Duration::ZERO));
        assert_eq!(parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), None);
        assert_eq!(parse_retry_after(Some("-1")), None);
        assert_eq!(parse_retry_after(Some("")), None);
        assert_eq!(parse_retry_after(None), None);
    }

    #[test]
    fn backoff_grows_stays_under_the_cap_and_honours_retry_after() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            respect_retry_after: true,
        };
        for (attempt, expected) in [(1u32, 100u128), (2, 200), (3, 400)] {
            let delay = policy.backoff_for(attempt, None).as_millis();
            assert!(
                delay >= expected / 2 && delay < expected,
                "attempt {attempt} gave {delay}ms, want [{}, {expected})",
                expected / 2
            );
        }
        assert!(policy.backoff_for(20, None) <= Duration::from_secs(1));
        assert_eq!(
            policy.backoff_for(1, Some(Duration::from_millis(900))),
            Duration::from_millis(900)
        );
        assert_eq!(
            policy.backoff_for(1, Some(Duration::from_secs(600))),
            Duration::from_secs(1),
            "Retry-After is capped by max_backoff"
        );
        let ignore = RetryPolicy { respect_retry_after: false, ..policy };
        assert!(ignore.backoff_for(1, Some(Duration::from_secs(600))) < Duration::from_millis(100));
    }

    #[test]
    fn jitter_actually_varies() {
        let policy = RetryPolicy {
            initial_backoff: Duration::from_millis(500),
            ..RetryPolicy::default()
        };
        let samples: std::collections::HashSet<u128> = (0..16)
            .map(|_| policy.backoff_for(1, None).as_nanos())
            .collect();
        assert!(samples.len() > 1, "backoff is not jittered at all");
    }

    #[test]
    fn zero_backoff_does_not_sleep() {
        let policy = RetryPolicy {
            initial_backoff: Duration::ZERO,
            ..RetryPolicy::default()
        };
        assert_eq!(policy.backoff_for(1, None), Duration::ZERO);
        assert_eq!(policy.backoff_for(9, None), Duration::ZERO);
    }

    #[test]
    fn constructors_set_the_attempt_count() {
        assert_eq!(RetryPolicy::none().max_attempts, 1);
        assert_eq!(RetryPolicy::attempts(5).max_attempts, 5);
        assert_eq!(RetryPolicy::default().max_attempts, 3);
    }
}

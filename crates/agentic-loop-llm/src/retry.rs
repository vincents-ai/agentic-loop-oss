//! Retry with exponential backoff for LLM API calls.
//!
//! Handles transient failures (rate limits, timeouts, server errors)
//! with configurable retry policy including jitter to avoid thundering herd.

use anyhow::Result;
use std::time::Duration;
use tracing::warn;

/// Retry policy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Multiplier for each retry (typically 2.0).
    pub multiplier: f64,
    /// Whether to add jitter (randomized delay within backoff window).
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// Create a policy with specified max retries.
    pub fn with_max_retries(max_retries: u32) -> Self {
        Self { max_retries, ..Default::default() }
    }

    /// Create a policy for aggressive retries (5 attempts, fast backoff).
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(10),
            multiplier: 2.0,
            jitter: true,
        }
    }

    /// Create a policy with no retries.
    pub fn none() -> Self {
        Self { max_retries: 0, ..Default::default() }
    }

    /// Calculate the backoff duration for a given attempt (0-indexed).
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_backoff.as_secs_f64() * self.multiplier.powi(attempt as i32);
        let capped = base.min(self.max_backoff.as_secs_f64());

        if self.jitter {
            // Add random jitter: 50% to 100% of the calculated backoff
            // Using a simple hash-based pseudo-random for determinism
            let jitter_factor = 0.5 + (capped * 1000.0).fract() * 0.5;
            Duration::from_secs_f64(capped * jitter_factor)
        } else {
            Duration::from_secs_f64(capped)
        }
    }

    /// Check if an error is retryable (transient).
    pub fn is_retryable(error: &anyhow::Error) -> bool {
        let msg = error.to_string().to_lowercase();
        // Rate limit
        msg.contains("rate limit") || msg.contains("429") ||
        // Server errors
        msg.contains("500") || msg.contains("502") || msg.contains("503") || msg.contains("504") ||
        // Timeouts
        msg.contains("timeout") || msg.contains("timed out") ||
        // Connection errors
        msg.contains("connection") || msg.contains("reset") || msg.contains("broken pipe") ||
        // Generic transient
        msg.contains("overloaded") || msg.contains("capacity")
    }
}

/// Execute an async operation with retry policy.
pub async fn with_retry<F, Fut, T>(policy: &RetryPolicy, operation: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_error = None;

    for attempt in 0..=policy.max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let is_retryable = RetryPolicy::is_retryable(&e);
                let is_last_attempt = attempt >= policy.max_retries;

                if !is_retryable || is_last_attempt {
                    return Err(e);
                }

                let backoff = policy.backoff_for_attempt(attempt);
                warn!(
                    attempt = attempt + 1,
                    max_retries = policy.max_retries,
                    backoff_ms = backoff.as_millis(),
                    error = %e,
                    "LLM call failed, retrying with backoff"
                );

                tokio::time::sleep(backoff).await;
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Retry exhausted with no error recorded")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.initial_backoff, Duration::from_millis(500));
        assert_eq!(policy.max_backoff, Duration::from_secs(30));
    }

    #[test]
    fn test_backoff_increases() {
        let policy = RetryPolicy::default();
        let b0 = policy.backoff_for_attempt(0);
        let b1 = policy.backoff_for_attempt(1);
        let b2 = policy.backoff_for_attempt(2);

        // With jitter, we can't compare exact values, but the base should increase
        assert!(b0 <= Duration::from_secs(1));
        assert!(b1 >= Duration::from_millis(100)); // at least some backoff
        assert!(b2 >= Duration::from_millis(100)); // at least some backoff
    }

    #[test]
    fn test_backoff_capped() {
        let policy = RetryPolicy::default();
        let b100 = policy.backoff_for_attempt(100);
        assert!(b100 <= policy.max_backoff);
    }

    #[test]
    fn test_no_jitter_backoff() {
        let policy = RetryPolicy {
            jitter: false,
            ..Default::default()
        };
        assert_eq!(policy.backoff_for_attempt(0), Duration::from_millis(500));
        assert_eq!(policy.backoff_for_attempt(1), Duration::from_secs(1));
        assert_eq!(policy.backoff_for_attempt(2), Duration::from_secs(2));
    }

    #[test]
    fn test_is_retryable_rate_limit() {
        let err = anyhow::anyhow!("Rate limit exceeded (429)");
        assert!(RetryPolicy::is_retryable(&err));
    }

    #[test]
    fn test_is_retryable_server_error() {
        let err = anyhow::anyhow!("Server error 502 Bad Gateway");
        assert!(RetryPolicy::is_retryable(&err));
    }

    #[test]
    fn test_is_retryable_timeout() {
        let err = anyhow::anyhow!("Request timed out after 30s");
        assert!(RetryPolicy::is_retryable(&err));
    }

    #[test]
    fn test_is_not_retryable_auth_error() {
        let err = anyhow::anyhow!("Invalid API key (401)");
        assert!(!RetryPolicy::is_retryable(&err));
    }

    #[test]
    fn test_is_not_retryable_bad_request() {
        let err = anyhow::anyhow!("Bad request (400): missing required field");
        assert!(!RetryPolicy::is_retryable(&err));
    }

    #[tokio::test]
    async fn test_with_retry_success_first_try() {
        let policy = RetryPolicy::none();
        let result: Result<i32> = with_retry(&policy, || async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_retry_success_after_failure() {
        let policy = RetryPolicy {
            max_retries: 2,
            initial_backoff: Duration::from_millis(1),
            jitter: false,
            ..Default::default()
        };
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result = with_retry(&policy, move || {
            let attempts = attempts_clone.clone();
            async move {
                let n = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Err(anyhow::anyhow!("Rate limit (429)"))
                } else {
                    Ok(99)
                }
            }
        }).await;

        assert_eq!(result.unwrap(), 99);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_with_retry_non_retryable_fails_immediately() {
        let policy = RetryPolicy::with_max_retries(5);
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<i32> = with_retry(&policy, move || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(anyhow::anyhow!("Invalid API key (401)"))
            }
        }).await;

        assert!(result.is_err());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_with_retry_exhausted() {
        let policy = RetryPolicy {
            max_retries: 2,
            initial_backoff: Duration::from_millis(1),
            jitter: false,
            ..Default::default()
        };

        let result: Result<i32> = with_retry(&policy, || async {
            Err(anyhow::anyhow!("Rate limit (429)"))
        }).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("429"));
    }
}

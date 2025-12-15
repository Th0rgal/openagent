//! LLM error types with retry classification.
//!
//! Distinguishes between transient errors (should retry) and permanent errors (should not retry).

use std::time::Duration;

/// Error from LLM API calls.
#[derive(Debug)]
pub struct LlmError {
    /// The kind of error
    pub kind: LlmErrorKind,
    /// HTTP status code, if applicable
    pub status_code: Option<u16>,
    /// Error message
    pub message: String,
    /// Suggested retry delay (from Retry-After header or calculated)
    pub retry_after: Option<Duration>,
}

impl LlmError {
    /// Create a rate limit error.
    pub fn rate_limited(message: String, retry_after: Option<Duration>) -> Self {
        Self {
            kind: LlmErrorKind::RateLimited,
            status_code: Some(429),
            message,
            retry_after,
        }
    }

    /// Create a server error.
    pub fn server_error(status_code: u16, message: String) -> Self {
        Self {
            kind: LlmErrorKind::ServerError,
            status_code: Some(status_code),
            message,
            retry_after: None,
        }
    }

    /// Create a client error (bad request, auth, etc.).
    pub fn client_error(status_code: u16, message: String) -> Self {
        Self {
            kind: LlmErrorKind::ClientError,
            status_code: Some(status_code),
            message,
            retry_after: None,
        }
    }

    /// Create a network error.
    pub fn network_error(message: String) -> Self {
        Self {
            kind: LlmErrorKind::NetworkError,
            status_code: None,
            message,
            retry_after: None,
        }
    }

    /// Create a parse error.
    pub fn parse_error(message: String) -> Self {
        Self {
            kind: LlmErrorKind::ParseError,
            status_code: None,
            message,
            retry_after: None,
        }
    }

    /// Check if this error is transient and should be retried.
    pub fn is_transient(&self) -> bool {
        self.kind.is_transient()
    }

    /// Get the suggested delay before retry.
    /// 
    /// Returns the `retry_after` if set, otherwise returns a default based on error kind.
    pub fn suggested_delay(&self, attempt: u32) -> Duration {
        if let Some(retry_after) = self.retry_after {
            return retry_after;
        }

        // Exponential backoff with jitter
        let base_delay = match self.kind {
            LlmErrorKind::RateLimited => Duration::from_secs(5), // Start higher for rate limits
            LlmErrorKind::ServerError => Duration::from_secs(2),
            LlmErrorKind::NetworkError => Duration::from_secs(1),
            _ => Duration::from_secs(1),
        };

        // Exponential backoff: base * 2^attempt, capped at 60 seconds
        let multiplier = 2u64.saturating_pow(attempt);
        let delay_secs = base_delay.as_secs().saturating_mul(multiplier).min(60);
        
        // Add jitter (up to 25% of delay)
        let jitter_range = delay_secs / 4;
        let jitter = if jitter_range > 0 {
            // Simple deterministic jitter based on attempt number
            (attempt as u64 * 7) % jitter_range
        } else {
            0
        };

        Duration::from_secs(delay_secs + jitter)
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status_code {
            Some(code) => write!(f, "{} (HTTP {}): {}", self.kind, code, self.message),
            None => write!(f, "{}: {}", self.kind, self.message),
        }
    }
}

impl std::error::Error for LlmError {}

/// Classification of LLM errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmErrorKind {
    /// Rate limited (429) - transient, should retry with backoff
    RateLimited,
    /// Server error (500, 502, 503, 504) - transient, should retry
    ServerError,
    /// Client error (400, 401, 403, 404) - permanent, should not retry
    ClientError,
    /// Network error (connection failed, timeout) - transient, should retry
    NetworkError,
    /// Response parsing error - usually permanent
    ParseError,
}

impl LlmErrorKind {
    /// Check if this error kind is transient (should retry).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            LlmErrorKind::RateLimited | LlmErrorKind::ServerError | LlmErrorKind::NetworkError
        )
    }
}

impl std::fmt::Display for LlmErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmErrorKind::RateLimited => write!(f, "Rate limited"),
            LlmErrorKind::ServerError => write!(f, "Server error"),
            LlmErrorKind::ClientError => write!(f, "Client error"),
            LlmErrorKind::NetworkError => write!(f, "Network error"),
            LlmErrorKind::ParseError => write!(f, "Parse error"),
        }
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Maximum total time to spend retrying
    pub max_retry_duration: Duration,
    /// Whether to retry on rate limit errors
    pub retry_rate_limits: bool,
    /// Whether to retry on server errors
    pub retry_server_errors: bool,
    /// Whether to retry on network errors
    pub retry_network_errors: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_retry_duration: Duration::from_secs(120), // 2 minutes max
            retry_rate_limits: true,
            retry_server_errors: true,
            retry_network_errors: true,
        }
    }
}

impl RetryConfig {
    /// Check if the given error should be retried based on this config.
    pub fn should_retry(&self, error: &LlmError) -> bool {
        match error.kind {
            LlmErrorKind::RateLimited => self.retry_rate_limits,
            LlmErrorKind::ServerError => self.retry_server_errors,
            LlmErrorKind::NetworkError => self.retry_network_errors,
            LlmErrorKind::ClientError | LlmErrorKind::ParseError => false,
        }
    }
}

/// Parse HTTP status code into error kind.
pub fn classify_http_status(status: u16) -> LlmErrorKind {
    match status {
        429 => LlmErrorKind::RateLimited,
        500 | 502 | 503 | 504 => LlmErrorKind::ServerError,
        400..=499 => LlmErrorKind::ClientError,
        _ => LlmErrorKind::ServerError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transient_classification() {
        assert!(LlmErrorKind::RateLimited.is_transient());
        assert!(LlmErrorKind::ServerError.is_transient());
        assert!(LlmErrorKind::NetworkError.is_transient());
        assert!(!LlmErrorKind::ClientError.is_transient());
        assert!(!LlmErrorKind::ParseError.is_transient());
    }

    #[test]
    fn test_http_status_classification() {
        assert_eq!(classify_http_status(429), LlmErrorKind::RateLimited);
        assert_eq!(classify_http_status(500), LlmErrorKind::ServerError);
        assert_eq!(classify_http_status(502), LlmErrorKind::ServerError);
        assert_eq!(classify_http_status(503), LlmErrorKind::ServerError);
        assert_eq!(classify_http_status(400), LlmErrorKind::ClientError);
        assert_eq!(classify_http_status(401), LlmErrorKind::ClientError);
        assert_eq!(classify_http_status(403), LlmErrorKind::ClientError);
    }

    #[test]
    fn test_exponential_backoff() {
        let error = LlmError::rate_limited("test".to_string(), None);
        
        let delay_0 = error.suggested_delay(0);
        let delay_1 = error.suggested_delay(1);
        let delay_2 = error.suggested_delay(2);

        // Should increase exponentially
        assert!(delay_1 > delay_0);
        assert!(delay_2 > delay_1);
        
        // Should be capped
        let delay_10 = error.suggested_delay(10);
        assert!(delay_10.as_secs() <= 60);
    }

    #[test]
    fn test_retry_after_respected() {
        let error = LlmError::rate_limited(
            "test".to_string(),
            Some(Duration::from_secs(30)),
        );
        
        // Should use the provided retry_after, not calculate
        assert_eq!(error.suggested_delay(0), Duration::from_secs(30));
        assert_eq!(error.suggested_delay(5), Duration::from_secs(30));
    }
}

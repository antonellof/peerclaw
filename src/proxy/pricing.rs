//! Pricing configuration for HTTP 402 proxy.

use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::wallet::to_micro;

/// Pricing for a specific endpoint pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointPricing {
    /// Path pattern (supports * and ** wildcards)
    pub pattern: String,
    /// HTTP method (or "*" for any)
    pub method: String,
    /// Price per request in μPCLAW
    pub price: u64,
    /// Description of this endpoint
    pub description: String,
}

impl EndpointPricing {
    /// Create a new endpoint pricing rule.
    pub fn new(pattern: &str, method: &str, price: u64, description: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            method: method.to_string(),
            price,
            description: description.to_string(),
        }
    }

    /// Check if a path/method matches this pricing rule.
    pub fn matches(&self, path: &str, method: &str) -> bool {
        // Check method
        if self.method != "*" && self.method.to_uppercase() != method.to_uppercase() {
            return false;
        }

        // Check path pattern
        self.pattern_matches(path)
    }

    /// Simple pattern matching with * and ** wildcards.
    fn pattern_matches(&self, path: &str) -> bool {
        let pattern = &self.pattern;

        // Exact match
        if pattern == path {
            return true;
        }

        // ** matches everything
        if pattern == "**" {
            return true;
        }

        // Split into segments
        let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut pi = 0; // pattern index
        let mut pa = 0; // path index

        while pi < pattern_parts.len() && pa < path_parts.len() {
            let pp = pattern_parts[pi];

            if pp == "**" {
                // ** matches zero or more segments
                // If ** is last pattern part, match rest of path
                if pi == pattern_parts.len() - 1 {
                    return true;
                }
                // Try to match remaining pattern with rest of path
                // Simplified: just check if next pattern part appears
                pi += 1;
                if pi < pattern_parts.len() {
                    while pa < path_parts.len() {
                        if pattern_parts[pi] == "*" || pattern_parts[pi] == path_parts[pa] {
                            break;
                        }
                        pa += 1;
                    }
                }
            } else if pp == "*" {
                // * matches exactly one segment
                pi += 1;
                pa += 1;
            } else if pp == path_parts[pa] {
                // Exact segment match
                pi += 1;
                pa += 1;
            } else {
                return false;
            }
        }

        // Both should be exhausted for a full match
        pi == pattern_parts.len() && pa == path_parts.len()
    }
}

/// Pricing configuration for the proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyPricing {
    /// Default price for unmatched endpoints
    pub default_price: u64,
    /// Endpoint-specific pricing rules (checked in order)
    pub endpoints: Vec<EndpointPricing>,
    /// Price multipliers by hour (0-23) for time-based pricing
    pub hourly_multipliers: HashMap<u8, f64>,
}

impl Default for ProxyPricing {
    fn default() -> Self {
        Self {
            default_price: to_micro(0.01), // 0.01 PCLAW default
            endpoints: vec![
                // Chat/inference endpoints (higher cost)
                EndpointPricing::new(
                    "/api/v1/chat/**",
                    "POST",
                    to_micro(0.1),
                    "Chat completion endpoint",
                ),
                EndpointPricing::new(
                    "/api/v1/completions",
                    "POST",
                    to_micro(0.1),
                    "Text completion endpoint",
                ),
                EndpointPricing::new(
                    "/v1/chat/completions",
                    "POST",
                    to_micro(0.1),
                    "OpenAI-compatible chat",
                ),
                // Embedding endpoints (medium cost)
                EndpointPricing::new(
                    "/api/v1/embeddings",
                    "POST",
                    to_micro(0.02),
                    "Embedding generation",
                ),
                EndpointPricing::new(
                    "/v1/embeddings",
                    "POST",
                    to_micro(0.02),
                    "OpenAI-compatible embeddings",
                ),
                // Image endpoints (higher cost)
                EndpointPricing::new(
                    "/api/v1/images/**",
                    "POST",
                    to_micro(0.5),
                    "Image generation",
                ),
                // Model info (free)
                EndpointPricing::new("/api/v1/models", "GET", 0, "List available models"),
                EndpointPricing::new("/v1/models", "GET", 0, "OpenAI-compatible model list"),
                // Health/status (free)
                EndpointPricing::new("/health", "*", 0, "Health check"),
                EndpointPricing::new("/", "GET", 0, "Index page"),
            ],
            hourly_multipliers: HashMap::new(),
        }
    }
}

impl ProxyPricing {
    /// Get the price for a request.
    pub fn get_price(&self, path: &str, method: &str) -> Option<u64> {
        // Find first matching endpoint
        for endpoint in &self.endpoints {
            if endpoint.matches(path, method) {
                let base_price = endpoint.price;

                // Apply hourly multiplier if configured
                let multiplier = self.get_hourly_multiplier();

                return Some((base_price as f64 * multiplier) as u64);
            }
        }

        // No match, use default
        Some(self.default_price)
    }

    /// Get the current hourly multiplier.
    fn get_hourly_multiplier(&self) -> f64 {
        if self.hourly_multipliers.is_empty() {
            return 1.0;
        }

        let hour = chrono::Utc::now().hour() as u8;
        *self.hourly_multipliers.get(&hour).unwrap_or(&1.0)
    }

    /// Set a custom endpoint pricing rule.
    pub fn add_endpoint(&mut self, endpoint: EndpointPricing) {
        // Insert at beginning to take priority
        self.endpoints.insert(0, endpoint);
    }

    /// Set hourly multiplier (for peak/off-peak pricing).
    pub fn set_hourly_multiplier(&mut self, hour: u8, multiplier: f64) {
        self.hourly_multipliers.insert(hour, multiplier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_path_match() {
        let endpoint = EndpointPricing::new("/api/v1/chat", "POST", 100, "test");

        assert!(endpoint.matches("/api/v1/chat", "POST"));
        assert!(!endpoint.matches("/api/v1/chat", "GET"));
        assert!(!endpoint.matches("/api/v1/other", "POST"));
    }

    #[test]
    fn test_wildcard_match() {
        let endpoint = EndpointPricing::new("/api/v1/*/info", "*", 100, "test");

        assert!(endpoint.matches("/api/v1/user/info", "GET"));
        assert!(endpoint.matches("/api/v1/agent/info", "POST"));
        assert!(!endpoint.matches("/api/v1/info", "GET")); // * requires one segment
    }

    #[test]
    fn test_double_wildcard() {
        let endpoint = EndpointPricing::new("/api/**", "*", 100, "test");

        assert!(endpoint.matches("/api/v1/chat", "GET"));
        assert!(endpoint.matches("/api/v1/chat/completions", "POST"));
        // Note: /api alone doesn't match /api/** because ** expects at least one segment after /api
        assert!(!endpoint.matches("/api", "GET"));
    }

    #[test]
    fn test_pricing_lookup() {
        let pricing = ProxyPricing::default();

        // Chat endpoint should be 0.1 PCLAW
        let price = pricing.get_price("/v1/chat/completions", "POST").unwrap();
        assert_eq!(price, to_micro(0.1));

        // Model list should be free
        let price = pricing.get_price("/v1/models", "GET").unwrap();
        assert_eq!(price, 0);

        // Unknown endpoint should use default
        let price = pricing.get_price("/unknown/path", "GET").unwrap();
        assert_eq!(price, to_micro(0.01));
    }
}

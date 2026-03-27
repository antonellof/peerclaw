//! SSRF (Server-Side Request Forgery) protection.
//!
//! Validates URLs before HTTP requests to prevent agents and tools from
//! accessing internal network resources, cloud metadata endpoints, or
//! other sensitive infrastructure.
//!
//! Two validation levels:
//! - **Strict** (`validate_url`): Blocks all private/internal ranges.
//! - **Relaxed** (`validate_url_relaxed`): Allows LAN but still blocks
//!   loopback, link-local, metadata endpoints, and unspecified addresses.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use thiserror::Error;

/// Errors returned by SSRF validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SsrfError {
    #[error("Invalid URL: {reason}")]
    InvalidUrl { reason: String },

    #[error("Blocked scheme: {scheme} (only http/https allowed)")]
    BlockedScheme { scheme: String },

    #[error("Blocked host: {host} ({reason})")]
    BlockedHost { host: String, reason: String },

    #[error("Blocked IP: {ip} ({reason})")]
    BlockedIp { ip: String, reason: String },

    #[error("DNS resolution failed for {host}: {reason}")]
    DnsResolutionFailed { host: String, reason: String },
}

/// Cloud metadata endpoints that must always be blocked.
const BLOCKED_HOSTNAMES: &[&str] = &[
    "metadata.google.internal",
    "metadata.google",
    "metadata",
    "instance-data",
];

/// Cloud metadata IP (AWS, GCP, Azure IMDS).
const METADATA_IPV4: Ipv4Addr = Ipv4Addr::new(169, 254, 169, 254);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate a URL with **strict** rules: blocks all private/internal IP ranges.
///
/// Call this before every outbound HTTP request from agent tools.
pub fn validate_url(url: &str) -> Result<(), SsrfError> {
    validate_url_inner(url, /* allow_lan */ false)
}

/// Validate a URL with **relaxed** rules: allows RFC 1918 LAN addresses
/// but still blocks loopback, link-local, metadata endpoints, and unspecified.
///
/// Use this for tools that legitimately need LAN access (e.g., accessing a
/// local NAS or printer).
pub fn validate_url_relaxed(url: &str) -> Result<(), SsrfError> {
    validate_url_inner(url, /* allow_lan */ true)
}

// ---------------------------------------------------------------------------
// Core validation
// ---------------------------------------------------------------------------

fn validate_url_inner(raw_url: &str, allow_lan: bool) -> Result<(), SsrfError> {
    // 1. Parse the URL
    let parsed = url::Url::parse(raw_url).map_err(|e| SsrfError::InvalidUrl {
        reason: e.to_string(),
    })?;

    // 2. Scheme check — only http and https
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SsrfError::BlockedScheme {
                scheme: other.to_string(),
            });
        }
    }

    // 3. Must have a host
    let host_str = parsed.host_str().ok_or_else(|| SsrfError::InvalidUrl {
        reason: "URL has no host".to_string(),
    })?;

    // 4. Block known metadata hostnames
    let host_lower = host_str.to_lowercase();
    for &blocked in BLOCKED_HOSTNAMES {
        if host_lower == blocked {
            return Err(SsrfError::BlockedHost {
                host: host_str.to_string(),
                reason: "cloud metadata endpoint".to_string(),
            });
        }
    }

    // 5. Try to parse the host as an IP directly
    if let Ok(ip) = host_str.parse::<IpAddr>() {
        check_ip(ip, host_str, allow_lan)?;
        return Ok(());
    }

    // 6. DNS resolution — resolve the hostname and check every resulting IP
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host_str, port);

    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| SsrfError::DnsResolutionFailed {
            host: host_str.to_string(),
            reason: e.to_string(),
        })?
        .collect();

    if addrs.is_empty() {
        return Err(SsrfError::DnsResolutionFailed {
            host: host_str.to_string(),
            reason: "no addresses returned".to_string(),
        });
    }

    for addr in &addrs {
        check_ip(addr.ip(), host_str, allow_lan)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// IP classification
// ---------------------------------------------------------------------------

/// Check whether a single IP address should be blocked.
fn check_ip(ip: IpAddr, host_display: &str, allow_lan: bool) -> Result<(), SsrfError> {
    match ip {
        IpAddr::V4(v4) => check_ipv4(v4, host_display, allow_lan),
        IpAddr::V6(v6) => check_ipv6(v6, host_display, allow_lan),
    }
}

fn check_ipv4(ip: Ipv4Addr, host_display: &str, allow_lan: bool) -> Result<(), SsrfError> {
    let octets = ip.octets();

    // Always blocked: loopback 127.0.0.0/8
    if octets[0] == 127 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "loopback address (127.0.0.0/8)".to_string(),
        });
    }

    // Always blocked: link-local 169.254.0.0/16
    if octets[0] == 169 && octets[1] == 254 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "link-local address (169.254.0.0/16)".to_string(),
        });
    }

    // Always blocked: unspecified 0.0.0.0/8
    if octets[0] == 0 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "unspecified address (0.0.0.0/8)".to_string(),
        });
    }

    // Always blocked: metadata IP specifically
    if ip == METADATA_IPV4 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "cloud metadata endpoint".to_string(),
        });
    }

    // RFC 1918 ranges — blocked in strict mode, allowed in relaxed
    if !allow_lan {
        // 10.0.0.0/8
        if octets[0] == 10 {
            return Err(SsrfError::BlockedIp {
                ip: ip.to_string(),
                reason: "private address (10.0.0.0/8)".to_string(),
            });
        }

        // 172.16.0.0/12 (172.16.x.x – 172.31.x.x)
        if octets[0] == 172 && (16..=31).contains(&octets[1]) {
            return Err(SsrfError::BlockedIp {
                ip: ip.to_string(),
                reason: "private address (172.16.0.0/12)".to_string(),
            });
        }

        // 192.168.0.0/16
        if octets[0] == 192 && octets[1] == 168 {
            return Err(SsrfError::BlockedIp {
                ip: ip.to_string(),
                reason: "private address (192.168.0.0/16)".to_string(),
            });
        }
    }

    // Extra check: block hostnames that resolved to metadata IP
    // (already caught above by link-local check, but be explicit)
    let _ = host_display;

    Ok(())
}

fn check_ipv6(ip: Ipv6Addr, _host_display: &str, allow_lan: bool) -> Result<(), SsrfError> {
    // Always blocked: loopback ::1
    if ip == Ipv6Addr::LOCALHOST {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "IPv6 loopback (::1)".to_string(),
        });
    }

    // Always blocked: unspecified ::
    if ip == Ipv6Addr::UNSPECIFIED {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "IPv6 unspecified (::)".to_string(),
        });
    }

    let segments = ip.segments();

    // Always blocked: link-local fe80::/10
    // fe80::/10 means first 10 bits are 1111_1110_10, i.e. segments[0] & 0xffc0 == 0xfe80
    if segments[0] & 0xffc0 == 0xfe80 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "IPv6 link-local (fe80::/10)".to_string(),
        });
    }

    // Unique local fc00::/7 — blocked in strict, allowed in relaxed
    // fc00::/7 means first 7 bits are 1111_110, i.e. segments[0] & 0xfe00 == 0xfc00
    if !allow_lan && segments[0] & 0xfe00 == 0xfc00 {
        return Err(SsrfError::BlockedIp {
            ip: ip.to_string(),
            reason: "IPv6 unique local (fc00::/7)".to_string(),
        });
    }

    // Check for IPv4-mapped IPv6 (::ffff:a.b.c.d)
    if let Some(v4) = ip.to_ipv4_mapped() {
        check_ipv4(v4, _host_display, allow_lan)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Strict mode tests ------------------------------------------------

    #[test]
    fn test_valid_public_url() {
        // We can't guarantee DNS resolution in CI, so test with an IP
        assert!(validate_url("http://8.8.8.8/path").is_ok());
        assert!(validate_url("https://1.1.1.1").is_ok());
    }

    #[test]
    fn test_blocks_non_http_schemes() {
        assert!(matches!(
            validate_url("ftp://example.com"),
            Err(SsrfError::BlockedScheme { .. })
        ));
        assert!(matches!(
            validate_url("file:///etc/passwd"),
            Err(SsrfError::BlockedScheme { .. })
        ));
        assert!(matches!(
            validate_url("gopher://evil.com"),
            Err(SsrfError::BlockedScheme { .. })
        ));
    }

    #[test]
    fn test_blocks_loopback_ipv4() {
        assert!(matches!(
            validate_url("http://127.0.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://127.0.0.2:8080/admin"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://127.255.255.255"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_loopback_ipv6() {
        assert!(matches!(
            validate_url("http://[::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_rfc1918_10() {
        assert!(matches!(
            validate_url("http://10.0.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://10.255.255.255/secret"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_rfc1918_172() {
        assert!(matches!(
            validate_url("http://172.16.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://172.31.255.255"),
            Err(SsrfError::BlockedIp { .. })
        ));
        // 172.15.x.x and 172.32.x.x are NOT private
        assert!(validate_url("http://172.15.0.1").is_ok());
        assert!(validate_url("http://172.32.0.1").is_ok());
    }

    #[test]
    fn test_blocks_rfc1918_192_168() {
        assert!(matches!(
            validate_url("http://192.168.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://192.168.255.255"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_link_local() {
        assert!(matches!(
            validate_url("http://169.254.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://169.254.169.254/latest/meta-data"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_unspecified() {
        assert!(matches!(
            validate_url("http://0.0.0.0"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://0.0.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_metadata_hostnames() {
        assert!(matches!(
            validate_url("http://metadata.google.internal/computeMetadata/v1/"),
            Err(SsrfError::BlockedHost { .. })
        ));
        assert!(matches!(
            validate_url("http://metadata.google/v1"),
            Err(SsrfError::BlockedHost { .. })
        ));
    }

    #[test]
    fn test_blocks_ipv6_link_local() {
        assert!(matches!(
            validate_url("http://[fe80::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_ipv6_unique_local_strict() {
        assert!(matches!(
            validate_url("http://[fd00::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://[fc00::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_ipv6_unspecified() {
        assert!(matches!(
            validate_url("http://[::]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_invalid_url() {
        assert!(matches!(
            validate_url("not a url"),
            Err(SsrfError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn test_no_host() {
        assert!(matches!(
            validate_url("http://"),
            Err(SsrfError::InvalidUrl { .. })
        ));
    }

    // ---- Relaxed mode tests -----------------------------------------------

    #[test]
    fn test_relaxed_allows_rfc1918() {
        assert!(validate_url_relaxed("http://10.0.0.1").is_ok());
        assert!(validate_url_relaxed("http://172.16.0.1").is_ok());
        assert!(validate_url_relaxed("http://192.168.1.100:8080").is_ok());
    }

    #[test]
    fn test_relaxed_still_blocks_loopback() {
        assert!(matches!(
            validate_url_relaxed("http://127.0.0.1"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url_relaxed("http://[::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_relaxed_still_blocks_link_local() {
        assert!(matches!(
            validate_url_relaxed("http://169.254.169.254"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_relaxed_still_blocks_metadata() {
        assert!(matches!(
            validate_url_relaxed("http://metadata.google.internal/v1"),
            Err(SsrfError::BlockedHost { .. })
        ));
    }

    #[test]
    fn test_relaxed_still_blocks_schemes() {
        assert!(matches!(
            validate_url_relaxed("ftp://192.168.1.1"),
            Err(SsrfError::BlockedScheme { .. })
        ));
    }

    #[test]
    fn test_relaxed_allows_ipv6_unique_local() {
        assert!(validate_url_relaxed("http://[fd00::1]").is_ok());
    }

    #[test]
    fn test_relaxed_still_blocks_ipv6_link_local() {
        assert!(matches!(
            validate_url_relaxed("http://[fe80::1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    // ---- IPv4-mapped IPv6 tests -------------------------------------------

    #[test]
    fn test_blocks_ipv4_mapped_ipv6_loopback() {
        assert!(matches!(
            validate_url("http://[::ffff:127.0.0.1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    #[test]
    fn test_blocks_ipv4_mapped_ipv6_private() {
        assert!(matches!(
            validate_url("http://[::ffff:10.0.0.1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
        assert!(matches!(
            validate_url("http://[::ffff:192.168.1.1]"),
            Err(SsrfError::BlockedIp { .. })
        ));
    }

    // ---- Error display tests ----------------------------------------------

    #[test]
    fn test_error_display() {
        let err = SsrfError::BlockedIp {
            ip: "127.0.0.1".to_string(),
            reason: "loopback address (127.0.0.0/8)".to_string(),
        };
        assert!(err.to_string().contains("127.0.0.1"));
        assert!(err.to_string().contains("loopback"));
    }
}

//! SSRF host guard for the web tools. An agent-chosen URL must never reach
//! loopback, link-local/cloud-metadata, or private-range targets: a fetched page
//! could otherwise steer the agent at internal services. Pure string/IP logic so
//! it is fully unit-testable; the fetch layer applies it to the initial URL and
//! to every redirect hop.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why `host` may not be fetched, or `None` when it is allowed. The check is
/// deliberately conservative: it blocks well-known internal names and every
/// non-public literal IP range. (Names that *resolve* to internal addresses are
/// not caught — that would require a custom resolver — so this is a guard rail,
/// not a sandbox.)
pub fn blocked_host_reason(host: &str) -> Option<&'static str> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(&host);

    if host.is_empty() {
        return Some("empty host");
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Some("loopback host");
    }
    if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".lan") {
        return Some("internal-network host");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => blocked_v4_reason(v4),
            IpAddr::V6(v6) => blocked_v6_reason(v6),
        };
    }
    None
}

fn blocked_v4_reason(ip: Ipv4Addr) -> Option<&'static str> {
    let [a, b, _, _] = ip.octets();
    match (a, b) {
        (127, _) => Some("loopback address"),
        (0, _) => Some("unspecified address"),
        (10, _) | (192, 168) => Some("private-range address"),
        (172, 16..=31) => Some("private-range address"),
        (169, 254) => Some("link-local/metadata address"),
        (100, 64..=127) => Some("carrier-grade NAT address"),
        _ => None,
    }
}

fn blocked_v6_reason(ip: Ipv6Addr) -> Option<&'static str> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return blocked_v4_reason(v4);
    }
    if ip.is_loopback() || ip.is_unspecified() {
        return Some("loopback address");
    }
    let first = ip.segments()[0];
    // fc00::/7 unique-local, fe80::/10 link-local.
    if first & 0xfe00 == 0xfc00 {
        return Some("private-range address");
    }
    if first & 0xffc0 == 0xfe80 {
        return Some("link-local/metadata address");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_private_and_metadata_hosts() {
        for host in [
            "localhost",
            "api.localhost",
            "printer.local",
            "vault.internal",
            "nas.lan",
            "127.0.0.1",
            "127.8.9.10",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "[::1]",
            "::ffff:192.168.0.1",
            "fe80::1",
            "fd00::2",
            "LOCALHOST.",
        ] {
            assert!(blocked_host_reason(host).is_some(), "{host} must be blocked");
        }
    }

    #[test]
    fn allows_public_hosts_and_addresses() {
        for host in [
            "example.com",
            "docs.rs",
            "sub.domain.co.uk",
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700::6810:84e5",
        ] {
            assert!(blocked_host_reason(host).is_none(), "{host} must be allowed");
        }
    }
}

use std::net::IpAddr;

use hd_run::{Grants, Limit};

use super::client::NetError;

pub(crate) fn at(grants: &Grants, host: &str, port: u16) -> Option<NetError> {
    let address = format!("{host}:{port}");
    if !valid_host(host) {
        Some(NetError::InvalidAddress { address })
    } else if !grants.covers_host("Net", host, Some(port)) {
        Some(NetError::NotGranted { address })
    } else {
        None
    }
}

#[must_use]
pub fn valid_host(host: &str) -> bool {
    if ip_literal(host) {
        return true;
    }
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    host.split('.').all(|label| {
        !label.is_empty()
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

pub(crate) fn ip_literal(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    if let Some((ip, zone)) = host.split_once('%')
        && ip.parse::<std::net::Ipv6Addr>().is_ok()
        && !zone.is_empty()
        && zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.:".contains(&b))
    {
        return true;
    }
    false
}

#[must_use]
pub fn covers_lookup(grants: &Grants, host: &str) -> bool {
    match grants.limit("Net") {
        None => true,
        Some(Limit::Deny) => false,
        Some(Limit::Entries(entries)) => entries.iter().any(|entry| {
            let name = if let Some(rest) = entry.strip_prefix('[') {
                rest.split_once(']')
                    .map_or(entry.as_str(), |(host, _)| host)
            } else if entry.bytes().filter(|b| *b == b':').count() == 1 {
                entry
                    .split_once(':')
                    .map_or(entry.as_str(), |(host, _)| host)
            } else {
                entry.as_str()
            };
            if let Some(suffix) = name.strip_prefix("*.") {
                host.strip_suffix(suffix)
                    .is_some_and(|head| head.ends_with('.') && head.len() > 1)
            } else {
                name.eq_ignore_ascii_case(host)
            }
        }),
    }
}

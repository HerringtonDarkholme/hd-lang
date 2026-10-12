use std::net::IpAddr;

use hd_run::{Grants, Limit};

use super::client::NetError;

pub(crate) fn at(grants: &Grants, host: &str, port: u16) -> Option<NetError> {
    let address = format!("{host}:{port}");
    if !valid_host(host) {
        Some(NetError::new(1, address))
    } else if !grants.covers_host("Net", host, Some(port)) {
        Some(NetError::new(0, address))
    } else {
        None
    }
}

pub(crate) fn valid_host(host: &str) -> bool {
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

pub(crate) fn covers_lookup(grants: &Grants, host: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{covers_lookup, valid_host};
    use crate::codec::Enc;
    use crate::net::client::Client;
    use hd_run::{Grants, Limit};

    #[test]
    fn names_literal_spelling_and_lookup_grants_match_node() {
        for valid in [
            "localhost",
            "a.",
            "a-b.example",
            "127.0.0.1",
            "::1",
            "fe80::1%lo0",
        ] {
            assert!(valid_host(valid), "{valid}");
        }
        for invalid in ["", "-a", "a-", "a..b", "a/b", "[::1]", "a\0b", "é.example"] {
            assert!(!valid_host(invalid), "{invalid}");
        }
        let grants = Grants {
            limits: vec![(
                "Net".into(),
                Limit::Entries(vec![
                    "127.0.0.1:80".into(),
                    "[::1]:9".into(),
                    "*.example:8".into(),
                ]),
            )],
        };
        assert!(covers_lookup(&grants, "127.0.0.1"));
        assert!(covers_lookup(&grants, "::1"));
        assert!(covers_lookup(&grants, "one.example"));
        assert!(!covers_lookup(&grants, "example"));
        assert!(!covers_lookup(&grants, "one.EXAMPLE"));
        let mut client = Client::default();
        for host in ["0:0:0:0:0:0:0:1", "fe80::1%lo0"] {
            let actual = client
                .call(
                    "lookup",
                    &[i64::try_from(host.len()).unwrap()],
                    host.as_bytes(),
                    &Grants::default(),
                )
                .unwrap();
            let mut expected = Enc::default();
            expected.leb(0).leb(1).str(host);
            assert_eq!(actual.done(), expected.done());
        }
    }
}

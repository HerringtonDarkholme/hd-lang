//! A request URL (`std-http.url.*`): an absolute `http` or `https` URL by
//! RFC 3986 with an ASCII host, parsed by `http::Uri`. A grant matches the
//! lowercase ASCII host; the host is mapped no other way, so there is no
//! IDNA table and no lookalike normalization. A redirect's `Location` is
//! joined to its base by RFC 3986 section 5.2 (`resolve`).

use std::fmt::Write;

use ureq::http::Uri;

/// A valid request URL, split into what the request needs.
pub struct Target {
    /// `https` rather than `http`.
    pub(crate) tls: bool,
    /// The host in lowercase, without the brackets of an IPv6 literal.
    pub host: String,
    /// The port, or the scheme's default.
    pub port: u16,
    /// The `Host` header's value: the host as written (lowercase, with
    /// brackets), and the port unless it is the scheme's default.
    pub authority: String,
    /// The URL's user and password as written, before percent-decoding.
    pub credentials: [String; 2],
    /// The path, dot segments removed (`/` when empty), and the query.
    pub path_and_query: String,
}

impl Target {
    /// `None` unless `value` is an absolute `http` or `https` URL with an
    /// ASCII host and a port within range. A fragment is dropped.
    #[must_use]
    pub fn parse(value: &str) -> Option<Target> {
        // RFC 3986 has no space, control or non-ASCII character, in the
        // host or elsewhere.
        if !value.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        let uri: Uri = value.parse().ok()?;
        let tls = match uri.scheme_str()? {
            "http" => false,
            "https" => true,
            _ => return None,
        };
        let authority = uri.authority()?;
        let host = authority.host();
        if host.is_empty() || host.contains('%') {
            return None;
        }
        let default = if tls { 443 } else { 80 };
        // What follows the host is empty or `:` and a port within range.
        let hostport = authority.as_str().rsplit('@').next().unwrap_or_default();
        let explicit = match hostport.get(host.len()..)? {
            "" => None,
            _ => Some(authority.port()?.as_u16()),
        };
        let port = explicit.unwrap_or(default);
        let host = host.to_ascii_lowercase();
        let mut shown = host.clone();
        if explicit.is_some_and(|port| port != default) {
            let _ = write!(shown, ":{port}");
        }
        let credentials = match authority.as_str().rsplit_once('@') {
            Some((userinfo, _)) => match userinfo.split_once(':') {
                Some((user, password)) => [user.into(), password.into()],
                None => [userinfo.into(), String::new()],
            },
            None => [String::new(), String::new()],
        };
        let mut path_and_query = remove_dot_segments(uri.path());
        if path_and_query.is_empty() {
            path_and_query.push('/');
        }
        if let Some(query) = uri.query() {
            path_and_query.push('?');
            path_and_query.push_str(query);
        }
        Some(Target {
            tls,
            host: host.trim_matches(['[', ']']).into(),
            port,
            authority: shown,
            credentials,
            path_and_query,
        })
    }

    /// The URL without credentials or fragment, as ureq takes it.
    #[must_use]
    pub fn uri(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}{}", self.authority, self.path_and_query)
    }
}

/// A reference split into the five components of RFC 3986 appendix B.
struct Parts<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

fn split(reference: &str) -> Parts<'_> {
    let (rest, fragment) = match reference.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (reference, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (scheme, rest) = match rest.split_once(':') {
        Some((scheme, rest)) if is_scheme(scheme) => (Some(scheme), rest),
        _ => (None, rest),
    };
    let (authority, path) = match rest.strip_prefix("//") {
        Some(rest) => {
            let end = rest.find('/').unwrap_or(rest.len());
            (Some(&rest[..end]), &rest[end..])
        }
        None => (None, rest),
    };
    Parts {
        scheme,
        authority,
        path,
        query,
        fragment,
    }
}

fn is_scheme(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Resolves `reference` against `base` (RFC 3986 section 5.2.2, the strict
/// parser) and recomposes the result (section 5.3). Neither is validated.
#[must_use]
pub fn resolve(base: &str, reference: &str) -> String {
    let base = split(base);
    let r = split(reference);
    let (scheme, authority, path, query);
    if let Some(s) = r.scheme {
        (scheme, authority, path, query) =
            (Some(s), r.authority, remove_dot_segments(r.path), r.query);
    } else if r.authority.is_some() {
        (scheme, authority, path, query) = (
            base.scheme,
            r.authority,
            remove_dot_segments(r.path),
            r.query,
        );
    } else if r.path.is_empty() {
        (scheme, authority, path, query) = (
            base.scheme,
            base.authority,
            base.path.to_owned(),
            r.query.or(base.query),
        );
    } else if r.path.starts_with('/') {
        (scheme, authority, path, query) = (
            base.scheme,
            base.authority,
            remove_dot_segments(r.path),
            r.query,
        );
    } else {
        let merged = merge(&base, r.path);
        (scheme, authority, path, query) = (
            base.scheme,
            base.authority,
            remove_dot_segments(&merged),
            r.query,
        );
    }
    let mut out = String::new();
    if let Some(scheme) = scheme {
        out.push_str(scheme);
        out.push(':');
    }
    if let Some(authority) = authority {
        out.push_str("//");
        out.push_str(authority);
    }
    out.push_str(&path);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    if let Some(fragment) = r.fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// RFC 3986 section 5.2.3.
fn merge(base: &Parts<'_>, path: &str) -> String {
    if base.authority.is_some() && base.path.is_empty() {
        return format!("/{path}");
    }
    match base.path.rfind('/') {
        Some(slash) => format!("{}{path}", &base.path[..=slash]),
        None => path.to_owned(),
    }
}

/// RFC 3986 section 5.2.4.
#[must_use]
pub fn remove_dot_segments(path: &str) -> String {
    let mut input = path;
    let mut output = String::new();
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest;
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") || input == "/.." {
            input = if input == "/.." { "/" } else { &input[3..] };
            output.truncate(output.rfind('/').unwrap_or(0));
        } else if input == "." || input == ".." {
            input = "";
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..].find('/').map_or(input.len(), |i| i + start);
            output.push_str(&input[..end]);
            input = &input[end..];
        }
    }
    output
}

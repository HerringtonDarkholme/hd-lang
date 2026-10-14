use super::{Headers, is_token, is_token_byte, parse_error};

#[derive(Default)]
pub(crate) enum HeaderPart {
    #[default]
    Status,
    Name,
    BeforeValue,
    Value,
}

/// Node's `TrackHeader` counts the reason phrase, field names and values,
/// rejecting at 16384, independently for each informational/final block.
/// Discard leading value whitespace as llhttp does; this also bounds the
/// retained header buffer without imposing an unrelated raw-byte cap.
#[derive(Default)]
pub(crate) struct HeaderScan {
    pub(crate) part: HeaderPart,
    size: usize,
    status_spaces: u8,
    status_prefix: usize,
    pub(crate) name_empty: bool,
    previous_cr: bool,
}

impl HeaderScan {
    pub(crate) fn byte(&mut self, byte: u8) -> Result<bool, ureq::Error> {
        if self.previous_cr && byte != b'\n' {
            return Err(parse_error(match self.part {
                HeaderPart::Status => "Expected LF after CR",
                _ => "Missing expected LF after header value",
            }));
        }
        if byte == b'\n' && !self.previous_cr {
            return Err(parse_error(match self.part {
                HeaderPart::Status => "Missing expected CR after response line",
                _ => "Missing expected CR after header value",
            }));
        }
        self.previous_cr = byte == b'\r';
        let counted = match self.part {
            HeaderPart::Status => {
                if byte == b'\n' {
                    self.part = HeaderPart::Name;
                    self.name_empty = true;
                    false
                } else if byte == b'\r' {
                    false
                } else if self.status_spaces >= 2 {
                    true
                } else {
                    self.status_prefix += 1;
                    if self.status_prefix > 20 {
                        return Err(parse_error("Invalid HTTP version"));
                    }
                    if byte == b' ' {
                        self.status_spaces += 1;
                    }
                    false
                }
            }
            HeaderPart::Name => {
                if byte == b'\r' || byte == b'\n' {
                    if !self.name_empty {
                        return Err(parse_error("Invalid header token"));
                    }
                    false
                } else if byte == b':' && !self.name_empty {
                    self.part = HeaderPart::BeforeValue;
                    false
                } else if matches!(byte, b' ' | b'\t') && self.name_empty {
                    return Err(parse_error("Unexpected whitespace after header value"));
                } else if !is_token_byte(byte) {
                    return Err(parse_error("Invalid header token"));
                } else {
                    self.name_empty = false;
                    true
                }
            }
            HeaderPart::BeforeValue if matches!(byte, b' ' | b'\t') => return Ok(false),
            HeaderPart::BeforeValue | HeaderPart::Value => {
                if byte == b'\n' {
                    self.part = HeaderPart::Name;
                    self.name_empty = true;
                    false
                } else if byte == b'\r' {
                    false
                } else {
                    if byte != 9 && (byte < 32 || byte == 127) {
                        return Err(parse_error("Invalid header value char"));
                    }
                    self.part = HeaderPart::Value;
                    true
                }
            }
        };
        self.size += usize::from(counted);
        if self.size >= 16_384 {
            return Err(parse_error("Header overflow"));
        }
        Ok(true)
    }
}

pub(crate) fn framing(headers: &Headers) -> Result<(Option<u64>, bool), ureq::Error> {
    let mut length = None;
    let mut transfer = false;
    let mut last_coding = "";
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(parse_error("Duplicate Content-Length"));
            }
            if transfer {
                return Err(parse_error(
                    "Content-Length can't be present with Transfer-Encoding",
                ));
            }
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(parse_error("Invalid character in Content-Length"));
            }
            length = Some(
                value
                    .parse()
                    .map_err(|_| parse_error("Content-Length overflow"))?,
            );
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if value.is_empty() {
                return Err(parse_error("Empty Transfer-Encoding"));
            }
            if length.is_some() {
                return Err(parse_error(
                    "Transfer-Encoding can't be present with Content-Length",
                ));
            }
            transfer = true;
            last_coding = value.split(',').next_back().unwrap_or_default().trim();
        }
    }
    Ok((length, last_coding.eq_ignore_ascii_case("chunked")))
}

pub(crate) fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
        .or_else(|| bytes.windows(2).position(|w| w == b"\n\n").map(|at| at + 2))
}

pub(crate) fn response_pairs(bytes: &[u8]) -> Result<(u16, Headers), ureq::Error> {
    let invalid = || {
        ureq::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Parse Error: Invalid header token",
        ))
    };
    let mut lines = bytes.split(|b| *b == b'\n');
    let first = lines.next().ok_or_else(invalid)?;
    if !first.starts_with(b"HTTP/1.0 ") && !first.starts_with(b"HTTP/1.1 ") {
        return Err(parse_error("Invalid HTTP version"));
    }
    let status = first
        .get(9..12)
        .filter(|bytes| bytes.iter().all(u8::is_ascii_digit))
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    let mut pairs: Headers = Vec::new();
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        let text = |bytes: &[u8]| bytes.iter().copied().map(char::from).collect::<String>();
        let trim = |bytes: &[u8]| {
            let start = bytes
                .iter()
                .position(|b| !matches!(b, b' ' | b'\t'))
                .unwrap_or(bytes.len());
            let end = bytes
                .iter()
                .rposition(|b| !matches!(b, b' ' | b'\t'))
                .map_or(start, |at| at + 1);
            text(&bytes[start..end])
        };
        if matches!(line.first(), Some(b' ' | b'\t')) {
            return Err(ureq::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Parse Error: Unexpected whitespace after header value",
            )));
        }
        let colon = line.iter().position(|b| *b == b':').ok_or_else(invalid)?;
        let name = std::str::from_utf8(&line[..colon]).map_err(|_| invalid())?;
        if !is_token(name)
            || line[colon + 1..]
                .iter()
                .any(|b| *b != 9 && (*b < 32 || *b == 127))
        {
            return Err(invalid());
        }
        pairs.push((name.into(), trim(&line[colon + 1..])));
    }
    Ok((status, pairs))
}

#[cfg(test)]
mod tests {
    use super::{HeaderScan, framing, response_pairs};
    use crate::http::test_support::{request, response, send, server};
    use hd_run::Grants;

    #[test]
    fn header_scanner_bounds_incomplete_headers_and_rejects_node_invalid_syntax() {
        let mut scan = HeaderScan::default();
        for byte in b"HTTP/1.1 200 OK\r\nX:" {
            scan.byte(*byte).unwrap();
        }
        for _ in 0..16_380 {
            scan.byte(b'a').unwrap();
        }
        assert_eq!(
            scan.byte(b'a').unwrap_err().to_string(),
            "io: Parse Error: Header overflow"
        );
        for (wire, reason) in [
            (
                b"HTTP/1.1 200 OK\n".as_slice(),
                "Missing expected CR after response line",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\n".as_slice(),
                "Missing expected CR after header value",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\rb".as_slice(),
                "Missing expected LF after header value",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\x01".as_slice(),
                "Invalid header value char",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\r\n\tcontinued".as_slice(),
                "Unexpected whitespace after header value",
            ),
        ] {
            let mut scan = HeaderScan::default();
            let error = wire
                .iter()
                .try_for_each(|byte| scan.byte(*byte).map(|_| ()))
                .unwrap_err();
            assert_eq!(error.to_string(), format!("io: Parse Error: {reason}"));
        }
        let mut scan = HeaderScan::default();
        for byte in b"HTTP/1.1 200 OK\r\nX:" {
            scan.byte(*byte).unwrap();
        }
        for _ in 0..100_000 {
            assert!(!scan.byte(b' ').unwrap());
        }
        assert_eq!(scan.size, 3);
    }

    #[test]
    fn original_framing_is_validated_before_reducing_headers() {
        for (headers, reason) in [
            (
                vec![
                    ("Content-Length".into(), "0".into()),
                    ("Content-Length".into(), "0".into()),
                ],
                "Duplicate Content-Length",
            ),
            (
                vec![
                    ("Content-Length".into(), "0".into()),
                    ("Transfer-Encoding".into(), "chunked".into()),
                ],
                "Transfer-Encoding can't be present with Content-Length",
            ),
            (
                vec![
                    ("Transfer-Encoding".into(), "chunked".into()),
                    ("Content-Length".into(), "0".into()),
                ],
                "Content-Length can't be present with Transfer-Encoding",
            ),
            (
                vec![("Content-Length".into(), "0, 0".into())],
                "Invalid character in Content-Length",
            ),
            (
                vec![("Transfer-Encoding".into(), String::new())],
                "Empty Transfer-Encoding",
            ),
        ] {
            assert_eq!(
                framing(&headers).unwrap_err().to_string(),
                format!("io: Parse Error: {reason}")
            );
        }
        assert_eq!(
            framing(&vec![("Transfer-Encoding".into(), "chunked, gzip".into())]).unwrap(),
            (None, false)
        );
        assert_eq!(
            framing(&vec![("Transfer-Encoding".into(), "gzip, chunked".into())]).unwrap(),
            (None, true)
        );
    }

    #[test]
    fn parser_preserves_raw_header_case_order_duplicates_and_latin1() {
        let (status, headers) = response_pairs(
            b"HTTP/1.1 500 Error\r\nX-A: first \t\r\nx-b: \xe9\r\nX-A: second\r\n\r\n",
        )
        .unwrap();
        assert_eq!(status, 500);
        assert_eq!(
            headers,
            [
                ("X-A".into(), "first".into()),
                ("x-b".into(), "é".into()),
                ("X-A".into(), "second".into())
            ]
        );
        assert!(response_pairs(b"HTTP/1.1 200 OK\r\nbad name: value\r\n\r\n").is_err());
    }

    #[test]
    fn client_accepts_over_128_headers_and_over_ten_mib_body() {
        let body = vec![0xaa; 10 * 1024 * 1024 + 17];
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..150 {
            wire.extend_from_slice(format!("X-{i}: {i}\r\n").as_bytes());
        }
        wire.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        wire.extend_from_slice(&body);
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 5000);
        let encoded = send(r.done(), &Grants::default());
        let (status, headers, received) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers.len(), 151);
        assert_eq!(headers[0], ("X-0".into(), "0".into()));
        assert_eq!(headers[149], ("X-149".into(), "149".into()));
        assert_eq!(received, body);
        server.join().unwrap();
    }

    #[test]
    fn client_keeps_all_1001_header_pairs_as_required_by_the_spec() {
        // Node v24.19 returns only the first 1000 pairs for this response.
        // std-http.headers.pairs requires all pairs; 2017 accounted header
        // bytes are well below the independent 16384-byte parser budget.
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        for _ in 0..1000 {
            wire.extend_from_slice(b"X: a\r\n");
        }
        wire.extend_from_slice(b"Content-Length: 0\r\n\r\n");
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = send(r.done(), &Grants::default());
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers.len(), 1001);
        assert!(
            headers[..1000]
                .iter()
                .all(|pair| pair == &("X".into(), "a".into()))
        );
        assert_eq!(headers[1000], ("Content-Length".into(), "0".into()));
        assert!(body.is_empty());
        server.join().unwrap();
    }
}

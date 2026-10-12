use std::collections::VecDeque;

use super::headers::{HeaderPart, HeaderScan};
use super::{is_token_byte, parse_error};

#[derive(Default)]
enum ChunkPart {
    #[default]
    Size,
    SizeLf,
    Data(u64),
    DataCr,
    DataLf,
    Trailers,
    End,
}

#[derive(Clone, Copy)]
enum ExtensionPart {
    Start,
    Name,
    ValueStart,
    Value,
    Quote,
    Escape,
    AfterQuote,
}

/// Validate Node's chunk framing and strip extensions before ureq sees
/// it: ureq-proto limits the entire size line to 20 bytes, even though a
/// valid extension may contain 16384 bytes. Body bytes stay untouched.
#[derive(Default)]
pub(crate) struct ChunkScan {
    part: ChunkPart,
    size: u64,
    digits: bool,
    extension: Option<ExtensionPart>,
    extension_size: usize,
    trailers: HeaderScan,
    trailer_line_size: usize,
}

impl ChunkScan {
    pub(crate) fn receive(
        &mut self,
        mut bytes: &[u8],
        output: &mut VecDeque<u8>,
    ) -> Result<(), ureq::Error> {
        while let Some((&byte, rest)) = bytes.split_first() {
            bytes = rest;
            match self.part {
                ChunkPart::Size => self.size_byte(byte)?,
                ChunkPart::SizeLf => {
                    if byte != b'\n' {
                        return Err(parse_error("Invalid character in chunk size"));
                    }
                    output.extend(format!("{:x}\r\n", self.size).bytes());
                    self.part = if self.size == 0 {
                        self.trailers.part = HeaderPart::Name;
                        self.trailers.name_empty = true;
                        ChunkPart::Trailers
                    } else {
                        ChunkPart::Data(self.size)
                    };
                }
                ChunkPart::Data(left) => {
                    output.push_back(byte);
                    let amount = usize::try_from(left - 1)
                        .unwrap_or(usize::MAX)
                        .min(bytes.len());
                    output.extend(&bytes[..amount]);
                    bytes = &bytes[amount..];
                    let left = left - 1 - u64::try_from(amount).unwrap_or(u64::MAX);
                    self.part = if left == 0 {
                        ChunkPart::DataCr
                    } else {
                        ChunkPart::Data(left)
                    };
                }
                ChunkPart::DataCr => {
                    if byte != b'\r' {
                        return Err(parse_error("Expected CR after chunk"));
                    }
                    output.push_back(byte);
                    self.part = ChunkPart::DataLf;
                }
                ChunkPart::DataLf => {
                    if byte != b'\n' {
                        return Err(parse_error("Expected LF after chunk"));
                    }
                    output.push_back(byte);
                    self.part = ChunkPart::Size;
                    self.size = 0;
                    self.digits = false;
                    self.extension = None;
                    self.extension_size = 0;
                }
                ChunkPart::Trailers => {
                    self.trailers.byte(byte)?;
                    if byte == b'\n' {
                        if self.trailer_line_size == 0 {
                            // The API exposes no trailers. Only release the
                            // framing terminator after validating all of the
                            // originals; ureq otherwise buffers a whole
                            // trailer line in its fixed-size input buffer.
                            output.extend(b"\r\n");
                            self.part = ChunkPart::End;
                        }
                        self.trailer_line_size = 0;
                    } else if byte != b'\r' {
                        self.trailer_line_size += 1;
                    }
                }
                ChunkPart::End => return Ok(()),
            }
        }
        Ok(())
    }

    fn size_byte(&mut self, byte: u8) -> Result<(), ureq::Error> {
        if byte == b'\r' {
            if !self.digits {
                return Err(parse_error("Invalid character in chunk size"));
            }
            if matches!(
                self.extension,
                Some(ExtensionPart::Start | ExtensionPart::Quote | ExtensionPart::Escape)
            ) {
                return Err(parse_error("Invalid character in chunk extensions"));
            }
            self.part = ChunkPart::SizeLf;
            return Ok(());
        }
        if let Some(part) = self.extension {
            self.extension_size += 1;
            if self.extension_size > 16_384 {
                return Err(parse_error("Chunk extensions overflow"));
            }
            self.extension = Some(match part {
                ExtensionPart::Start if is_token_byte(byte) => ExtensionPart::Name,
                ExtensionPart::Name if is_token_byte(byte) => ExtensionPart::Name,
                ExtensionPart::Name if byte == b'=' => ExtensionPart::ValueStart,
                ExtensionPart::Name
                | ExtensionPart::ValueStart
                | ExtensionPart::Value
                | ExtensionPart::AfterQuote
                    if byte == b';' =>
                {
                    ExtensionPart::Start
                }
                ExtensionPart::ValueStart if byte == b'"' => ExtensionPart::Quote,
                ExtensionPart::ValueStart | ExtensionPart::Value if is_token_byte(byte) => {
                    ExtensionPart::Value
                }
                ExtensionPart::Quote if byte == b'"' => ExtensionPart::AfterQuote,
                ExtensionPart::Quote if byte == b'\\' => ExtensionPart::Escape,
                ExtensionPart::Quote | ExtensionPart::Escape
                    if byte == b'\t' || byte >= 32 && byte != 127 =>
                {
                    ExtensionPart::Quote
                }
                _ => {
                    return Err(parse_error(match part {
                        ExtensionPart::Start => "Invalid character in chunk extensions",
                        ExtensionPart::Name => "Invalid character in chunk extensions name",
                        ExtensionPart::AfterQuote => {
                            "Invalid character in chunk extensions quote value"
                        }
                        _ => "Invalid character in chunk extensions value",
                    }));
                }
            });
        } else if byte == b';' && self.digits {
            self.extension = Some(ExtensionPart::Start);
        } else if let Some(digit) = char::from(byte)
            .to_digit(16)
            .filter(|_| byte.is_ascii_hexdigit())
        {
            self.size = self
                .size
                .checked_mul(16)
                .and_then(|n| n.checked_add(u64::from(digit)))
                .ok_or_else(|| parse_error("Chunk size overflow"))?;
            self.digits = true;
        } else {
            return Err(parse_error("Invalid character in chunk size"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ChunkScan;
    use crate::codec::Enc;
    use crate::http::Client;
    use crate::http::test_support::{request, response, server};
    use hd_run::Grants;

    #[test]
    fn chunk_extensions_are_streamed_validated_and_normalized_without_changing_body() {
        let raw = b"3;foo=01234567890123456789;quoted=\"a b\"\r\nabc\r\n2;empty=;flag\r\nde\r\n0\r\nX-Trailer: yes\r\n\r\n";
        let mut chunks = ChunkScan::default();
        let mut output = std::collections::VecDeque::new();
        for byte in raw {
            chunks.receive(&[*byte], &mut output).unwrap();
        }
        assert_eq!(
            output.into_iter().collect::<Vec<_>>(),
            b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
        );
        for (raw, reason) in [
            (b"Z\r\n".as_slice(), "Invalid character in chunk size"),
            (
                b"3;\r\n".as_slice(),
                "Invalid character in chunk extensions",
            ),
            (
                b"3;f=\x01\r\n".as_slice(),
                "Invalid character in chunk extensions value",
            ),
        ] {
            let error = ChunkScan::default()
                .receive(raw, &mut std::collections::VecDeque::new())
                .unwrap_err();
            assert_eq!(error.to_string(), format!("io: Parse Error: {reason}"));
        }
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        wire.extend_from_slice(raw);
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        assert_eq!(response(encoded.done()).2, b"abcde");
        server.join().unwrap();
    }

    #[test]
    fn valid_long_leading_trailer_whitespace_does_not_fill_ureq_buffers() {
        let mut wire =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-Trailer:"
                .to_vec();
        wire.extend(std::iter::repeat_n(b' ', 100_000));
        wire.extend_from_slice(b"yes\r\n\r\n");
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers, [("Transfer-Encoding".into(), "chunked".into())]);
        assert_eq!(body, b"abc");
        server.join().unwrap();
    }

    #[test]
    fn client_decodes_http10_chunked_like_node_and_rejects_invalid_framing() {
        let (url, first_server) = server(
            b"HTTP/1.0 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n".to_vec(),
        );
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        assert_eq!(
            response(encoded.done()),
            (
                200,
                vec![("Transfer-Encoding".into(), "chunked".into())],
                b"abc".to_vec()
            )
        );
        first_server.join().unwrap();

        let (url, server) =
            server(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let mut expected = Enc::default();
        expected
            .leb(1)
            .leb(3)
            .str("Parse Error: Duplicate Content-Length");
        assert_eq!(encoded.done(), expected.done());
        server.join().unwrap();
    }
}

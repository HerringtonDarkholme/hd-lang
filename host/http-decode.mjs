import { gunzipSync, inflateSync } from "node:zlib";

export const MAX_DECODED_BYTES = 64 * 1024 * 1024;

const err = (variant, text = null) => ({ error: variant, text });
export const hasNoBody = (method, status) =>
  method === "HEAD" || (status >= 100 && status < 200) || status === 204 || status === 304;

function contentCodings(headers) {
  const fields = headers.filter(([name]) => name.toLowerCase() === "content-encoding");
  const codings = [];
  for (const [, value] of fields) {
    for (const part of value.split(",")) {
      const coding = part.trim().toLowerCase();
      if (!coding || !["gzip", "deflate", "identity"].includes(coding)) {
        return err("InvalidContentEncoding", coding || value);
      }
      codings.push(coding);
    }
  }
  return { fields, codings };
}

// Produces the one representation that crosses the host ABI. Validate the
// complete coding stack before transforming any bytes, then undo codings in
// reverse order. Node's gzip decoder accepts concatenated members and checks
// every member trailer.
export function decodedResponse(method, status, headers, encoded) {
  if (hasNoBody(method, status)) {
    return { status, headers, body: Buffer.alloc(0) };
  }
  const parsed = contentCodings(headers);
  if (parsed.error) {
    return parsed;
  }
  const { fields, codings } = parsed;
  let body = encoded;
  for (const coding of [...codings].reverse()) {
    if (coding === "identity") {
      continue;
    }
    try {
      const options = { maxOutputLength: MAX_DECODED_BYTES + 1 };
      body = coding === "gzip" ? gunzipSync(body, options) : inflateSync(body, options);
    } catch (error) {
      if (error?.code === "ERR_BUFFER_TOO_LARGE") {
        return err("OutputTooLarge");
      }
      return err("InvalidContentEncoding", coding);
    }
    if (body.length > MAX_DECODED_BYTES) {
      return err("OutputTooLarge");
    }
  }
  if (body.length > MAX_DECODED_BYTES) {
    return err("OutputTooLarge");
  }
  const normalized = fields.length
    ? headers.filter(
        ([name]) => !["content-encoding", "content-length"].includes(name.toLowerCase()),
      )
    : headers;
  return { status, headers: normalized, body };
}

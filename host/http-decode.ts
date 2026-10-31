import { gunzipSync, inflateSync } from "node:zlib";

export const MAX_DECODED_BYTES = 64 * 1024 * 1024;

export type Header = [string, string];
export interface HttpFailure {
  error: string;
  text: string | null;
  status?: never;
  headers?: never;
  body?: never;
}
export interface DecodedResponse {
  status: number;
  headers: Header[];
  body: Buffer;
  error?: never;
  text?: never;
}

type Coding = "gzip" | "deflate" | "identity";
interface CodingStack {
  fields: Header[];
  codings: Coding[];
}
interface ErrorWithCode {
  code?: string;
}

const err = (variant: string, text: string | null = null): HttpFailure => ({
  error: variant,
  text,
});
export const hasNoBody = (method: string, status: number): boolean =>
  method === "HEAD" || (status >= 100 && status < 200) || status === 204 || status === 304;

function contentCodings(headers: Header[]): CodingStack | HttpFailure {
  const fields = headers.filter(([name]) => name.toLowerCase() === "content-encoding");
  const codings: Coding[] = [];
  for (const [, value] of fields) {
    for (const part of value.split(",")) {
      const coding = part.trim().toLowerCase();
      if (!coding || !["gzip", "deflate", "identity"].includes(coding)) {
        return err("InvalidContentEncoding", coding || value);
      }
      codings.push(coding as Coding);
    }
  }
  return { fields, codings };
}

// Produces the one representation that crosses the host ABI. Validate the
// complete coding stack before transforming any bytes, then undo codings in
// reverse order. Node's gzip decoder accepts concatenated members and checks
// every member trailer.
export function decodedResponse(
  method: string,
  status: number,
  headers: Header[],
  encoded: Buffer,
): DecodedResponse | HttpFailure {
  if (hasNoBody(method, status)) {
    return { status, headers, body: Buffer.alloc(0) };
  }
  const parsed = contentCodings(headers);
  if ("error" in parsed) {
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
      if ((error as ErrorWithCode)?.code === "ERR_BUFFER_TOO_LARGE") {
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

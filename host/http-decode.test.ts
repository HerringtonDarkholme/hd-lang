import assert from "node:assert/strict";
import { test } from "node:test";
import { deflateRawSync, deflateSync, gzipSync } from "node:zlib";
import { MAX_DECODED_BYTES, decodedResponse } from "./http-decode.ts";

test("decoded HTTP output validates and reverses the complete coding stack", () => {
  const original = Buffer.from("stacked body");
  const encoded = deflateSync(gzipSync(original));
  const headers = [
    ["content-encoding", "gzip"],
    ["content-encoding", "deflate, identity"],
    ["content-length", String(encoded.length)],
    ["x-test", "kept"],
  ];
  const decoded = decodedResponse("GET", 200, headers, encoded);
  assert.deepEqual(decoded.body, original);
  assert.deepEqual(decoded.headers, [["x-test", "kept"]]);
});

test("decoded HTTP output accepts concatenated gzip members", () => {
  const encoded = Buffer.concat([gzipSync("first "), gzipSync("second")]);
  const decoded = decodedResponse("GET", 200, [["content-encoding", "gzip"]], encoded);
  assert.equal(decoded.body.toString(), "first second");
});

test("decoded HTTP output rejects unsupported, corrupt, and raw deflate bodies", () => {
  assert.deepEqual(
    decodedResponse("GET", 200, [["content-encoding", "br, gzip"]], gzipSync("body")),
    { error: "InvalidContentEncoding", text: "br" },
  );

  const corrupt = gzipSync("body");
  assert.deepEqual(
    decodedResponse("GET", 200, [["content-encoding", "gzip"]], corrupt.subarray(0, 4)),
    { error: "InvalidContentEncoding", text: "gzip" },
  );
  assert.deepEqual(
    decodedResponse("GET", 200, [["content-encoding", "deflate"]], deflateRawSync("body")),
    { error: "InvalidContentEncoding", text: "deflate" },
  );
});

test("bodyless HTTP responses do not inspect an encoded payload", () => {
  const headers = [
    ["content-encoding", "gzip"],
    ["content-length", "18"],
  ];
  assert.deepEqual(decodedResponse("HEAD", 200, headers, Buffer.from("not a gzip stream")), {
    status: 200,
    headers,
    body: Buffer.alloc(0),
  });
});

test("decoded HTTP output accepts the 64 MiB boundary and rejects the next byte", () => {
  const headers = [
    ["content-encoding", "identity"],
    ["content-length", String(MAX_DECODED_BYTES)],
    ["x-test", "kept"],
  ];
  const exact = decodedResponse("GET", 200, headers, Buffer.alloc(MAX_DECODED_BYTES));
  assert.equal(exact.body.length, MAX_DECODED_BYTES);
  assert.deepEqual(exact.headers, [["x-test", "kept"]]);

  assert.deepEqual(decodedResponse("GET", 200, [], Buffer.alloc(MAX_DECODED_BYTES + 1)), {
    error: "OutputTooLarge",
    text: null,
  });

  assert.deepEqual(
    decodedResponse(
      "GET",
      200,
      [["content-encoding", "gzip"]],
      gzipSync(Buffer.alloc(MAX_DECODED_BYTES + 1)),
    ),
    { error: "OutputTooLarge", text: null },
  );
});

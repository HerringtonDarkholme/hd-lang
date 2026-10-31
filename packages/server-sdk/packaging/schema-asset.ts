import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
const hash = (text) => createHash("sha256").update(text).digest("hex");
const bad = (message) => new Error(`SDK server schema: ${message}`);
const hex = /^[0-9a-f]{64}$/;
export function schemaFingerprint(snapshot) {
  const { fingerprint: _fingerprint, ...payload } = snapshot;
  return hash(JSON.stringify(payload));
}
function freeze(value) {
  if (value && typeof value === "object") {
    for (const nested of Object.values(value)) {
      freeze(nested);
    }
    Object.freeze(value);
  }
  return value;
}
const validMetadata = (snapshot) =>
  snapshot?.format === 1 &&
  snapshot.packageHostABI === 1 &&
  hex.test(snapshot.fingerprint ?? "") &&
  schemaFingerprint(snapshot) === snapshot.fingerprint;
const validPayload = (snapshot) =>
  snapshot.schema?.root === "server.HttpServer" &&
  Array.isArray(snapshot.schema.interfaces) &&
  snapshot.schema.interfaces.length === Object.keys(snapshot.coverage ?? {}).length &&
  Boolean(snapshot.provenance) &&
  hex.test(snapshot.provenance.compilerSha256 ?? "") &&
  hex.test(snapshot.provenance.referenceWasmSha256 ?? "") &&
  hex.test(snapshot.provenance.referenceContractSha256 ?? "");
export function validateSchemaAsset(snapshot) {
  if (!validMetadata(snapshot)) {
    throw bad("expected schema metadata or checksum mismatch");
  }
  if (!validPayload(snapshot)) {
    throw bad("invalid expected schema payload");
  }
  return freeze(snapshot);
}
export async function readServerSchema(
  file = new URL("../runtime/server-schema.json", import.meta.url),
) {
  let snapshot;
  try {
    snapshot = JSON.parse(await readFile(file, "utf8"));
  } catch {
    throw bad("cannot read expected schema JSON");
  }
  return validateSchemaAsset(snapshot);
}

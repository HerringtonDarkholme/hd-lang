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
    for (const v of Object.values(value)) {
      freeze(v);
    }
    Object.freeze(value);
  }
  return value;
}
export function validateSchemaAsset(snapshot) {
  if (
    snapshot?.format !== 1 ||
    snapshot.packageHostABI !== 1 ||
    !hex.test(snapshot.fingerprint ?? "") ||
    schemaFingerprint(snapshot) !== snapshot.fingerprint
  ) {
    throw bad("expected schema metadata or checksum mismatch");
  }
  if (
    snapshot.schema?.root !== "server.HttpServer" ||
    !Array.isArray(snapshot.schema.interfaces) ||
    snapshot.schema.interfaces.length !== Object.keys(snapshot.coverage ?? {}).length ||
    !snapshot.provenance ||
    !hex.test(snapshot.provenance.compilerSha256 ?? "") ||
    !hex.test(snapshot.provenance.referenceWasmSha256 ?? "") ||
    !hex.test(snapshot.provenance.referenceContractSha256 ?? "")
  ) {
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

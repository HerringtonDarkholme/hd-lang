// Share links carry the whole project in the URL hash:
//   #code=<base64url(UTF-8 source)>                  one file, src/main.hd
//   #project=<base64url(UTF-8 JSON {files, main})>   any project
// base64url is RFC 4648 section 5 without padding.

import { asProject, DEFAULT_MAIN, type Project } from "./project.ts";

export function encodeBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function decodeBase64Url(encoded: string): string {
  const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return new TextDecoder("utf-8", { fatal: true }).decode(
    Uint8Array.from(binary, (character) => character.charCodeAt(0)),
  );
}

/** The hash that reproduces `project`: `#code=` when it is only src/main.hd. */
export function projectHash(project: Project): string {
  const paths = Object.keys(project.files);
  if (paths.length === 1 && paths[0] === DEFAULT_MAIN && project.main === DEFAULT_MAIN)
    return `#code=${encodeBase64Url(project.files[DEFAULT_MAIN]!)}`;
  return `#project=${encodeBase64Url(JSON.stringify({ files: project.files, main: project.main }))}`;
}

/** Reads a `#code=` or `#project=` hash; undefined for any other or a malformed one. */
export function projectFromHash(hash: string): Project | undefined {
  const match = /^#?(code|project)=([A-Za-z0-9_-]*)$/.exec(hash.trim());
  if (!match) return undefined;
  try {
    const text = decodeBase64Url(match[2]!);
    if (match[1] === "code") return { files: { [DEFAULT_MAIN]: text }, main: DEFAULT_MAIN };
    return asProject(JSON.parse(text));
  } catch {
    return undefined;
  }
}

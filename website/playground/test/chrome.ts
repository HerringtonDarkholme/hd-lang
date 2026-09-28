// Finds a local Chromium for the end-to-end scripts (website/e2e.ts and
// website/playground/e2e.ts). CHROME_PATH wins; otherwise a local Chrome, Edge, or
// Playwright Chromium is used.

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export function browserPath(): string {
  const candidates = [
    process.env.CHROME_PATH,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    join(
      homedir(),
      "Library/Caches/ms-playwright/chromium-1208/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    ),
  ];
  const found = candidates.find((candidate) => candidate && existsSync(candidate));
  if (!found) throw new Error("no Chromium found; set CHROME_PATH");
  return found;
}

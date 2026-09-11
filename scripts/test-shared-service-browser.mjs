// Optional real Chromium check for the live fixture in test-shared-services.py.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.ZORK_PLAYWRIGHT_MODULE || "playwright");
const browser = await chromium.launch({ executablePath: process.env.ZORK_SERVICE_CHROMIUM || undefined, headless: true });
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(process.argv[2]);
  await page.locator("#api").filter({ hasText: "ok" }).waitFor();
  await page.locator("#ws").filter({ hasText: "websocket-ok" }).waitFor();
  await page.evaluate(() => {
    document.cookie = "private_preview=first; Path=/";
    localStorage.setItem("private_preview", "first");
  });
  const other = await context.newPage();
  await other.goto(process.argv[3]);
  await other.locator("#api").filter({ hasText: "ok" }).waitFor();
  assert.equal(await other.evaluate(() => document.cookie.includes("private_preview=first")), false);
  assert.equal(await other.evaluate(() => localStorage.getItem("private_preview")), null);
  if (process.argv[4]) await page.screenshot({ path: process.argv[4] });
  console.log("PASS Chromium: localhost resolution, page/API/WebSocket, service cookie/storage isolation");
} finally {
  await browser.close();
}

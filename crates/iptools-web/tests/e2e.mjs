import assert from "node:assert/strict";
import crypto from "node:crypto";
import { chromium, firefox, webkit } from "playwright";

const baseURL = process.env.IPTOOLS_WEB_URL ?? "http://127.0.0.1:8099/";
const browserName = process.env.IPTOOLS_BROWSER ?? "chromium";
const browserType = { chromium, firefox, webkit }[browserName];
assert.ok(browserType, `Unsupported browser: ${browserName}`);
const launchOptions = {
  headless: true,
  ...(browserName === "chromium" && !process.env.CI ? { channel: "chrome" } : {}),
};
const browser = await browserType.launch(launchOptions);

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));

  await page.goto(`${baseURL}?scenario=wifi-degraded&lang=zh`, {
    waitUntil: "networkidle",
  });
  await page.waitForSelector("#terminal_ratzilla_grid");
  await page.waitForTimeout(600);
  await page.evaluate(() => document.fonts.ready);
  await page.locator("#terminal").focus();
  assert.equal(
    await page.evaluate(() => document.fonts.check('16px "Maple Mono CN iptools"')),
    true,
    "Bundled CJK terminal font should be loaded",
  );
  await page.screenshot({
    path: "../../target/playwright-web-demo-font-fixed.png",
    fullPage: true,
  });

  assert.equal(
    await page.evaluate(() => localStorage.getItem("iptools.web.v1.scenario")),
    "wifi-degraded",
  );
  if (browserName === "chromium") {
    assert.equal(await page.evaluate(() => navigator.serviceWorker.ready.then(() => true)), true);
  }

  const before = hash(await page.locator("#terminal").screenshot());
  await page.keyboard.press("Tab");
  await page.waitForTimeout(150);
  const afterTab = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(afterTab, before, "Tab should switch the rendered page");

  await page.keyboard.press("Tab");
  await page.keyboard.press("Space");
  await page.waitForTimeout(900);
  const scanning = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(scanning, afterTab, "Scanner progress should update the terminal");

  await page.getByRole("button", { name: "F1" }).click();
  await page.waitForTimeout(100);
  const help = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(help, scanning, "Touch controls should reach the shared reducer");

  const foreignOrigins = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .map((entry) => new URL(entry.name).origin)
      .filter((origin) => origin !== location.origin),
  );
  assert.deepEqual(foreignOrigins, []);
  assert.deepEqual(pageErrors, []);

  const canvasPage = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  await canvasPage.goto(`${baseURL}?renderer=canvas&lang=en&scenario=home-network`, {
    waitUntil: "networkidle",
  });
  await canvasPage.waitForSelector("#terminal canvas");
  await canvasPage.locator("#terminal").focus();
  const canvasBefore = hash(await canvasPage.locator("#terminal").screenshot());
  await canvasPage.keyboard.press("Tab");
  await canvasPage.waitForTimeout(150);
  assert.notEqual(
    hash(await canvasPage.locator("#terminal").screenshot()),
    canvasBefore,
    "Canvas keyboard input should reach the shared reducer",
  );
  await canvasPage.close();

  console.log(`iptools web e2e: ${browserName} Chinese DOM, Canvas and interaction passed`);
} finally {
  await browser.close();
}

function hash(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

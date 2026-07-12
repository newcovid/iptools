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
    waitUntil: "domcontentloaded",
  });
  await page.waitForSelector("#terminal_ratzilla_grid");
  await page.waitForTimeout(600);
  await page.evaluate(() => document.fonts.ready);
  assert.match(
    await page.locator("#terminal").textContent(),
    /\[Ctrl\+C\].*(退出|Quit)/,
    "DOM terminal size must keep the bottom footer inside the visible grid",
  );
  assert.match(await page.locator(".demo-badge").textContent(), /v0\.4 PREVIEW.*SIMULATED DATA/);
  assert.ok(
    (await page.locator(".demo-badge").boundingBox())?.height <= 24,
    "Preview badge should remain on one line",
  );
  await page.locator("#terminal").focus();
  assert.equal(await page.evaluate(() => document.activeElement?.id), "terminal");
  assert.equal(
    await page.evaluate(() => document.fonts.check('16px "Maple Mono CN iptools"')),
    true,
    "Bundled CJK terminal font should be loaded",
  );
  await page.screenshot({
    path: "../../target/playwright-web-demo-font-fixed.png",
    fullPage: false,
  });

  assert.equal(
    await page.evaluate(() => localStorage.getItem("iptools.web.v1.scenario")),
    "wifi-degraded",
  );
  if (browserName === "chromium") {
    assert.equal(await page.evaluate(() => navigator.serviceWorker.ready.then(() => true)), true);
  }

  const dashboardGeneration = Number(
    await page.locator("#terminal").getAttribute("data-rendered-input-generation"),
  );
  await page.keyboard.press("r");
  await page.waitForFunction(
    (generation) =>
      Number(document.getElementById("terminal")?.dataset.renderedInputGeneration) > generation,
    dashboardGeneration,
  );
  await page.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("198.51.100.27") && text.includes("公网信息");
  });

  const before = hash(await page.locator("#terminal").screenshot());
  await page.keyboard.press("Tab");
  await page.waitForTimeout(150);
  const afterTab = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(afterTab, before, "Tab should switch the rendered page");
  await page.keyboard.press("r");
  await page.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("192.168.50.37") && text.includes("适配器详情");
  });
  await page.waitForTimeout(250);
  await page.screenshot({
    path: "../../target/playwright-web-adapter-zh.png",
    fullPage: false,
  });

  // Adapter Edit uses the same reducer and renderer as native demo, but its
  // runtime is deterministic and must never call browser or network APIs.
  await page.keyboard.press("e");
  await page.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("编辑适配器"),
  );
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Home");
  for (let index = 0; index < 15; index += 1) {
    await page.keyboard.press("Delete");
  }
  await page.keyboard.type("10.20.30.40");
  await page.keyboard.press("Enter");
  await page.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("确认应用此网络配置"),
  );
  await page.screenshot({
    path: "../../target/playwright-web-adapter-edit-zh.png",
    fullPage: false,
  });
  await page.keyboard.press("Enter");
  await page.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("模拟配置已应用"),
  );
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("10.20.30.40") && text.includes("适配器详情");
  });

  await page.keyboard.press("Tab");
  await page.keyboard.press("Space");
  await page.waitForTimeout(900);
  const scanning = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(scanning, afterTab, "Scanner progress should update the terminal");

  await page.getByRole("button", { name: "F1" }).click();
  await page.waitForTimeout(100);
  const help = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(help, scanning, "Touch controls should reach the shared reducer");
  assert.equal(
    await page.evaluate(() => document.activeElement?.id),
    "terminal",
    "Touch controls should restore terminal focus",
  );

  const wheelGeneration = Number(
    await page.locator("#terminal").getAttribute("data-rendered-input-generation"),
  );
  const terminalBox = await page.locator("#terminal").boundingBox();
  assert.ok(terminalBox);
  await page.mouse.move(terminalBox.x + terminalBox.width / 2, terminalBox.y + terminalBox.height / 2);
  await page.mouse.wheel(0, 120);
  await page.waitForFunction(
    (generation) =>
      Number(document.getElementById("terminal")?.dataset.renderedInputGeneration) > generation,
    wheelGeneration,
  );

  for (const zoom of ["80%", "100%", "125%", "150%"]) {
    await page.evaluate((value) => {
      document.body.style.zoom = value;
    }, zoom);
    await page.waitForTimeout(100);
    const box = await page.locator("#terminal").boundingBox();
    assert.ok(box && box.width > 300 && box.height > 200, `Terminal should remain visible at ${zoom}`);
  }
  await page.evaluate(() => {
    document.body.style.zoom = "100%";
  });

  if (browserName === "chromium") {
    await page.getByRole("button", { name: "Fullscreen" }).click();
    await page.waitForFunction(() => document.fullscreenElement !== null);
    await page.getByRole("button", { name: "Exit fullscreen" }).click();
    await page.waitForFunction(() => document.fullscreenElement === null);
  }

  const foreignOrigins = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .map((entry) => new URL(entry.name).origin)
      .filter((origin) => origin !== location.origin),
  );
  assert.deepEqual(foreignOrigins, []);
  assert.deepEqual(pageErrors, []);

  if (browserName === "chromium") {
    assert.equal(
      await page.evaluate(async () => {
        await navigator.serviceWorker.ready;
        return (await caches.keys()).some((name) =>
          name.startsWith("iptools-web-v0.4-alpha.1-"),
        );
      }),
      true,
      "The current offline cache should be active",
    );
    await page.context().setOffline(true);
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#terminal_ratzilla_grid");
    assert.match(await page.locator(".demo-badge").textContent(), /SIMULATED DATA/);
    await page.context().setOffline(false);
  }

  const canvasPage = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  await canvasPage.goto(`${baseURL}?renderer=canvas&lang=en&scenario=home-network`, {
    waitUntil: "domcontentloaded",
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
  const latencies = await measureInputLatencies(canvasPage, 40);
  const p95 = percentile(latencies, 0.95);
  assert.ok(p95 <= 100, `Canvas input latency p95 should be <=100ms, got ${p95.toFixed(1)}ms`);
  await canvasPage.close();

  const chineseCanvas = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  const chineseCanvasErrors = [];
  chineseCanvas.on("pageerror", (error) => chineseCanvasErrors.push(error.message));
  await chineseCanvas.goto(`${baseURL}?renderer=canvas&lang=zh&scenario=multi-adapter`, {
    waitUntil: "domcontentloaded",
  });
  await chineseCanvas.waitForSelector("#terminal canvas");
  await chineseCanvas.evaluate(() => document.fonts.ready);
  assert.equal(
    await chineseCanvas.evaluate(() => document.fonts.check('16px "Maple Mono CN iptools"')),
    true,
  );
  assert.deepEqual(chineseCanvasErrors, []);
  await chineseCanvas.close();

  const settingsPage = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  await settingsPage.goto(`${baseURL}?renderer=dom&lang=en`, {
    waitUntil: "domcontentloaded",
  });
  await settingsPage.waitForSelector("#terminal_ratzilla_grid");
  await settingsPage.evaluate(() => {
    localStorage.setItem("iptools.web.v1.scan_concurrency", "50");
  });
  await settingsPage.reload({ waitUntil: "domcontentloaded" });
  await settingsPage.waitForSelector("#terminal_ratzilla_grid");
  await settingsPage.locator("#terminal").focus();
  const nextPageButton = settingsPage.getByRole("button", { name: "Tab", exact: true });
  for (let index = 0; index < 5; index += 1) {
    await nextPageButton.click();
  }
  await settingsPage.waitForFunction(
    () => document.getElementById("terminal")?.textContent?.includes("Reset remembered parameters"),
  );
  await settingsPage.keyboard.press("ArrowDown");
  await settingsPage.keyboard.press("ArrowRight");
  await settingsPage.waitForFunction(
    () => localStorage.getItem("iptools.web.v1.scan_concurrency") === "60",
  );
  await settingsPage.waitForFunction(() =>
    /Scan concurrency\s*:\s*60/.test(document.getElementById("terminal")?.textContent ?? ""),
  );
  assert.match(await settingsPage.locator("#terminal").textContent(), /Scan concurrency\s*:\s*60/);
  await settingsPage.keyboard.press("ArrowDown");
  await settingsPage.keyboard.press("Enter");
  await settingsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("Cleared"),
  );
  await settingsPage.close();

  const trafficPage = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await trafficPage.goto(`${baseURL}?scenario=multi-adapter&lang=en&renderer=dom`, {
    waitUntil: "domcontentloaded",
  });
  await trafficPage.waitForSelector("#terminal");
  await trafficPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("Dashboard"),
  );
  const trafficNextPage = trafficPage.getByRole("button", { name: "Tab", exact: true });
  for (let index = 0; index < 3; index += 1) {
    const generation = Number(
      await trafficPage.locator("#terminal").getAttribute("data-rendered-input-generation"),
    );
    await trafficNextPage.click();
    await trafficPage.waitForFunction(
      (previous) =>
        Number(document.getElementById("terminal")?.dataset.renderedInputGeneration) > previous,
      generation,
    );
  }
  await trafficPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("Live Traffic"),
  );
  const beforeTrafficRefresh = Number(
    await trafficPage.locator("#terminal").getAttribute("data-rendered-state-revision"),
  );
  await trafficPage.keyboard.press("r");
  await trafficPage.waitForFunction(
    (revision) =>
      Number(document.getElementById("terminal")?.dataset.renderedStateRevision) > revision &&
      document.getElementById("terminal")?.textContent?.includes("Ethernet"),
    beforeTrafficRefresh,
  );
  await trafficPage.close();

  const diagnosticsPage = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await diagnosticsPage.goto(`${baseURL}?scenario=home-network&lang=zh&renderer=dom`, {
    waitUntil: "domcontentloaded",
  });
  await diagnosticsPage.waitForSelector("#terminal_ratzilla_grid");
  await diagnosticsPage.locator("#terminal").focus();
  for (let index = 0; index < 4; index += 1) {
    await diagnosticsPage.keyboard.press("Tab");
  }
  await diagnosticsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("按 Enter 进入诊断工具"),
  );
  await diagnosticsPage.keyboard.press("Enter");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await waitForStableTerminal(diagnosticsPage);
  const diagnosticMenuFocus = hash(await diagnosticsPage.locator("#terminal").screenshot());
  await diagnosticsPage.keyboard.press("ArrowDown");
  await waitForStableTerminal(diagnosticsPage);
  assert.notEqual(
    hash(await diagnosticsPage.locator("#terminal").screenshot()),
    diagnosticMenuFocus,
    "Shift+Tab should return diagnostic focus to the tool menu",
  );
  await diagnosticsPage.keyboard.press("ArrowUp");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() =>
    /reply 1: \d+ ms/.test(document.getElementById("terminal")?.textContent ?? ""),
  );
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("ArrowDown");
  const beforeIntervalAdjust = Number(
    await diagnosticsPage.locator("#terminal").getAttribute("data-rendered-input-generation"),
  );
  await diagnosticsPage.keyboard.press("ArrowRight");
  await diagnosticsPage.waitForFunction(
    (generation) =>
      Number(document.getElementById("terminal")?.dataset.renderedInputGeneration) > generation,
    beforeIntervalAdjust,
  );
  await diagnosticsPage.keyboard.press("ArrowUp");
  await diagnosticsPage.keyboard.press("Control+r");
  await diagnosticsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("目标历史"),
  );
  await diagnosticsPage.keyboard.press("Escape");
  await diagnosticsPage.keyboard.press("Escape");
  await diagnosticsPage.keyboard.press("Enter");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("192.0.2.1"),
  );
  await waitForStableTerminal(diagnosticsPage);
  await diagnosticsPage.screenshot({
    path: "../../target/playwright-web-ping-trace-zh.png",
    fullPage: false,
  });
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("ArrowRight");
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("HomeLab") && text.includes("评级") && text.includes("完成");
  });
  await waitForStableTerminal(diagnosticsPage);
  await diagnosticsPage.screenshot({
    path: "../../target/playwright-web-link-quality-zh.png",
    fullPage: false,
  });
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("demo.invalid") && text.includes("Mbps") && text.includes("完成");
  });
  await waitForStableTerminal(diagnosticsPage);
  await diagnosticsPage.screenshot({
    path: "../../target/playwright-web-public-link-zh.png",
    fullPage: false,
  });
  await diagnosticsPage.close();

  const narrowPage = await browser.newPage({ viewport: { width: 390, height: 844 } });
  await narrowPage.goto(`${baseURL}?lang=zh`, { waitUntil: "domcontentloaded" });
  assert.notEqual(
    await narrowPage.locator(".rotate").evaluate((element) => getComputedStyle(element).display),
    "none",
    "Portrait layouts should display the landscape hint",
  );
  await narrowPage.close();

  console.log(`iptools web e2e: ${browserName} DOM, Canvas, offline and interaction passed`);
} finally {
  await browser.close();
}

function hash(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

async function waitForStableTerminal(page) {
  await page.waitForFunction(() => {
    const terminal = document.getElementById("terminal");
    return (
      terminal?.dataset.pendingStateRevision !== undefined &&
      terminal.dataset.pendingStateRevision === terminal.dataset.renderedStateRevision
    );
  });
  let previous = null;
  let stableFrames = 0;
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const current = hash(await page.locator("#terminal").screenshot());
    if (current === previous) {
      stableFrames += 1;
      if (stableFrames >= 2) return;
    } else {
      previous = current;
      stableFrames = 0;
    }
    await page.waitForTimeout(50);
  }
  assert.fail("Terminal did not reach a stable rendered frame");
}

async function measureInputLatencies(page, iterations) {
  return page.evaluate(async (count) => {
    const terminal = document.getElementById("terminal");
    const samples = [];
    for (let index = 0; index < count; index += 1) {
      const previous = terminal.dataset.renderedInputGeneration;
      const start = performance.now();
      await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          observer.disconnect();
          reject(new Error("render acknowledgement timed out"));
        }, 1000);
        const observer = new MutationObserver(() => {
          if (terminal.dataset.renderedInputGeneration === previous) return;
          clearTimeout(timeout);
          observer.disconnect();
          resolve();
        });
        observer.observe(terminal, {
          attributes: true,
          attributeFilter: ["data-rendered-input-generation"],
        });
        terminal.dispatchEvent(
          new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
        );
      });
      samples.push(performance.now() - start);
    }
    return samples;
  }, iterations);
}

function percentile(values, fraction) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)];
}

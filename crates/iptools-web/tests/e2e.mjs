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
  await isolateStorage(page);
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
  assert.equal(await page.locator("html").getAttribute("lang"), "zh-CN");
  assert.equal(await page.locator("html").getAttribute("data-theme"), "classic");
  assert.match(await page.locator(".exhibit-intro").textContent(), /浏览器.*演示场景/);
  assert.match(await page.locator(".demo-badge:visible").textContent(), /模拟数据/);
  assert.ok(
    (await page.locator(".demo-badge:visible").boundingBox())?.height <= 24,
    "Version badge should remain on one line",
  );
  await page.locator("#terminal").focus();
  assert.equal(await page.evaluate(() => document.activeElement?.id), "terminal");
  assert.equal(
    await page.evaluate(() => document.fonts.check('16px "Maple Mono CN iptools"')),
    true,
    "Bundled CJK terminal font should be loaded",
  );
  await page.keyboard.press("Control+l");
  await page.waitForFunction(() => document.documentElement.lang === "en");
  assert.match(await page.locator(".exhibit-intro").textContent(), /Experience iptools/);
  await page.keyboard.press("Control+l");
  await page.waitForFunction(() => document.documentElement.lang === "zh-CN");
  assert.match(await page.locator(".exhibit-intro").textContent(), /浏览器.*演示场景/);
  assert.equal(
    await page.locator("#terminal_ratzilla_grid span").evaluateAll((elements) =>
      elements.some((element) => getComputedStyle(element).color === "rgb(19, 161, 14)"),
    ),
    true,
    "Classic Web green should match the native Windows Terminal palette",
  );
  const firstClock = (await page.locator("#terminal").textContent()).match(
    /20\d\d-\d\d-\d\d \d\d:\d\d:\d\d/,
  )?.[0];
  assert.ok(firstClock, "Dashboard should render a local clock");
  await page.waitForFunction(
    (previous) => {
      const current = document
        .getElementById("terminal")
        ?.textContent?.match(/20\d\d-\d\d-\d\d \d\d:\d\d:\d\d/)?.[0];
      return current && current !== previous;
    },
    firstClock,
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
    return text.includes("198.51.100.27") && text.includes("公网连接信息");
  });

  const before = hash(await page.locator("#terminal").screenshot());
  await page.keyboard.press("Tab");
  await page.waitForTimeout(150);
  const afterTab = hash(await page.locator("#terminal").screenshot());
  assert.notEqual(afterTab, before, "Tab should switch the rendered page");
  await page.keyboard.press("r");
  await page.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("192.168.50.37") && text.includes("详细信息");
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
    return text.includes("10.20.30.40") && text.includes("详细信息");
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
  await page.locator('button[data-action="reset"]').click();
  await page.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("field-laptop.demo") && text.includes("198.51.100.27");
  });
  assert.equal(await page.locator("#scenario-select").inputValue(), "wifi-degraded");

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
    await page.locator("#fullscreen-button").click();
    await page.waitForFunction(() => document.fullscreenElement !== null);
    await page.locator("#fullscreen-button").click();
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
          name.startsWith("iptools-web-v0.4.0-"),
        );
      }),
      true,
      "The current offline cache should be active",
    );
    await page.context().setOffline(true);
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#terminal_ratzilla_grid");
    assert.match(await page.locator(".demo-badge:visible").textContent(), /模拟数据/);
    await page.context().setOffline(false);
  }

  const canvasPage = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  await isolateStorage(canvasPage);
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
  await isolateStorage(chineseCanvas);
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
  await chineseCanvas.goto(`${baseURL}?lang=zh&scenario=multi-adapter`, {
    waitUntil: "domcontentloaded",
  });
  await chineseCanvas.waitForSelector("#terminal canvas");
  assert.equal(
    await chineseCanvas.evaluate(() => localStorage.getItem("iptools.web.v1.renderer")),
    "canvas",
    "Stored renderer should apply when no renderer URL parameter is present",
  );
  await chineseCanvas.close();

  const mousePage = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await isolateStorage(mousePage);
  await mousePage.goto(`${baseURL}?scenario=home-network&lang=zh&renderer=dom`, {
    waitUntil: "domcontentloaded",
  });
  await mousePage.waitForSelector("#terminal_ratzilla_grid");
  await clickTerminalCell(mousePage, 33, 1);
  await mousePage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("工具列表"),
  );
  await clickTerminalCell(mousePage, 2, 6);
  await mousePage.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("端口扫描") && text.includes("起始端口");
  });
  assert.match(
    await mousePage.locator("#terminal").textContent(),
    /端口扫描.*起始端口/s,
    "A single mouse press should switch and focus the clicked diagnostic tool",
  );
  await mousePage.close();

  const settingsPage = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  await isolateStorage(settingsPage);
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
  await settingsPage.keyboard.press("ArrowRight");
  await settingsPage.waitForFunction(() => {
    const saved = JSON.parse(localStorage.getItem("iptools.web.v1.config") ?? "{}");
    return saved.theme === "nord";
  });
  await settingsPage.waitForFunction(
    () => document.documentElement.dataset.theme === "nord",
  );
  assert.equal(
    await settingsPage.locator(".stage").evaluate((element) => getComputedStyle(element).borderColor),
    "rgb(76, 86, 106)",
    "The Web shell should follow the selected Nord theme",
  );
  await settingsPage.waitForFunction(() =>
    [...document.querySelectorAll("#terminal_ratzilla_grid span")].some((element) =>
      /^rgba?\(46, 52, 64(?:, 1)?\)$/.test(getComputedStyle(element).backgroundColor),
    ),
  );
  await settingsPage.reload({ waitUntil: "domcontentloaded" });
  await settingsPage.waitForSelector("#terminal_ratzilla_grid");
  assert.equal(await settingsPage.locator("html").getAttribute("data-theme"), "nord");
  assert.match(await settingsPage.locator("#terminal").textContent(), /Color theme\s*:\s*Nord/);
  for (let index = 0; index < 3; index += 1) {
    await settingsPage.keyboard.press("ArrowDown");
  }
  await settingsPage.keyboard.press("Enter");
  await settingsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("Cleared"),
  );
  await settingsPage.close();

  const trafficPage = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await isolateStorage(trafficPage);
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
    document.getElementById("terminal")?.textContent?.includes("Real-time Monitor"),
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
  await isolateStorage(diagnosticsPage);
  await diagnosticsPage.goto(`${baseURL}?scenario=home-network&lang=zh&renderer=dom`, {
    waitUntil: "domcontentloaded",
  });
  await diagnosticsPage.waitForSelector("#terminal_ratzilla_grid");
  await diagnosticsPage.locator("#terminal").focus();
  for (let index = 0; index < 4; index += 1) {
    await diagnosticsPage.keyboard.press("Tab");
  }
  await diagnosticsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("工具列表"),
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
    /回复 seq=1 bytes=\d+ ttl=\d+ time=\d+ms/.test(
      document.getElementById("terminal")?.textContent ?? "",
    ),
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
  await diagnosticsPage.evaluate(() => {
    window.__iptoolsCtrlRPrevented = false;
    document.addEventListener(
      "keydown",
      (event) => {
        if (event.ctrlKey && event.key.toLowerCase() === "r") {
          window.__iptoolsCtrlRPrevented = event.defaultPrevented;
        }
      },
    );
  });
  await diagnosticsPage.keyboard.press("Control+r");
  await diagnosticsPage.waitForFunction(() =>
    document.getElementById("terminal")?.textContent?.includes("目标历史"),
  );
  assert.equal(
    await diagnosticsPage.evaluate(() => window.__iptoolsCtrlRPrevented),
    true,
    "Ctrl+R must open history without allowing the browser reload action",
  );
  await diagnosticsPage.keyboard.press("Escape");
  await diagnosticsPage.getByRole("button", { name: "Ctrl+R" }).click();
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
  // Port Scan keeps its tool-specific stats/table/progress/status layout while the
  // deterministic Web runtime supplies typed progress and open-port events.
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("443") && text.includes("HTTPS") && text.includes("完成");
  });
  await waitForStableTerminal(diagnosticsPage);
  await diagnosticsPage.screenshot({
    path: "../../target/playwright-web-port-scan-zh.png",
    fullPage: false,
  });

  await diagnosticsPage.keyboard.press("Shift+Tab");
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
  // LAN Speed exercises its dynamic client configuration (mode and
  // peer editing) before starting the same reducer/render path as native demo.
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("ArrowDown");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("Tab");
  await diagnosticsPage.keyboard.press("ArrowRight");
  for (let index = 0; index < 4; index += 1) {
    await diagnosticsPage.keyboard.press("ArrowDown");
  }
  await diagnosticsPage.keyboard.press("Home");
  for (let index = 0; index < 32; index += 1) {
    await diagnosticsPage.keyboard.press("Delete");
  }
  await diagnosticsPage.keyboard.type("192.0.2.25");
  await diagnosticsPage.keyboard.press("Shift+Tab");
  await diagnosticsPage.keyboard.press("Space");
  await diagnosticsPage.waitForFunction(() => {
    const text = document.getElementById("terminal")?.textContent ?? "";
    return text.includes("192.0.2.25:50505") && text.includes("Mbps") && text.includes("完成");
  });
  await waitForStableTerminal(diagnosticsPage);
  await diagnosticsPage.screenshot({
    path: "../../target/playwright-web-lan-speed-zh.png",
    fullPage: false,
  });
  assert.ok(
    await diagnosticsPage.evaluate(() => {
      const saved = JSON.parse(localStorage.getItem("iptools.web.v1.config") ?? "{}");
      return Array.isArray(saved.session?.history?.targets) && saved.session.history.targets.length > 0;
    }),
    "Diagnostic target history should persist through the same shared session update",
  );
  await diagnosticsPage.close();

  const narrowPage = await browser.newPage({ viewport: { width: 390, height: 844 } });
  await isolateStorage(narrowPage);
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
  // The dashboard clock intentionally changes once per second, so pixel hashes
  // cannot be a reliable stability signal on slower browsers or CI runners.
  // The renderer's revision handshake is the authoritative frame boundary.
  await page.waitForTimeout(100);
}

async function isolateStorage(page) {
  await page.addInitScript(() => {
    if (sessionStorage.getItem("iptools.e2e.initialized")) return;
    localStorage.clear();
    sessionStorage.setItem("iptools.e2e.initialized", "1");
  });
}

async function clickTerminalCell(page, column, row) {
  const terminal = await page.locator("#terminal_ratzilla_grid").boundingBox();
  const cell = await page.locator("#terminal_ratzilla_grid span").first().boundingBox();
  assert.ok(terminal && cell, "DOM terminal cell geometry should be measurable");
  await page.mouse.click(
    terminal.x + cell.width * (column + 0.5),
    terminal.y + cell.height * (row + 0.5),
  );
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

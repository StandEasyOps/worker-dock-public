import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { chromium } from "@playwright/test";

const REPO = new URL("../docs/screenshots/ui-mvp", import.meta.url).pathname;
const PORT = 4173;
const shots = [
  ["ready", "ready.png", null],
  ["connected", "connected.png", null],
  ["not-ready", "not-ready.png", null],
  ["amber", "amber.png", null],
  ["unsigned", "not-signed-in.png", null],
  ["error", "error.png", null],
  ["empty", "empty-folders.png", "[data-state='folders-empty']"],
  ["folders-two", "folders-two.png", ".folders li:last-child"],
  ["service-missing", "service-not-installed.png", "#service-heading"],
  ["service-stopped", "service-installed-stopped.png", "#service-heading"],
  ["service-autostart", "service-running-autostart.png", "#service-heading"],
  ["direct", "direct-running.png", "#direct-heading"],
  ["settings", "settings.png", "#about"],
  ["logs", "logs.png", null],
  ["name", "name-field.png", "#worker-name"],
  ["recent-agents", "recent-agents.png", "#recent-agents-heading"],
];

await mkdir(REPO, { recursive: true });

const preview = spawn("npx", ["vite", "preview", "--host", "127.0.0.1", "--port", String(PORT), "--strictPort"], {
  stdio: "pipe",
});

async function waitForServer() {
  for (let i = 0; i < 40; i += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${PORT}/`);
      if (response.ok || response.status === 404) {
        return;
      }
    } catch {
      // still booting
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("vite preview did not start");
}

try {
  await waitForServer();
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 960, height: 680 } });
  for (const [mode, file, scroll] of shots) {
    await page.goto(`http://127.0.0.1:${PORT}/?preview=${mode}`, { waitUntil: "networkidle" });
    await page.waitForSelector(".lamp, h1");
    await page.evaluate(() => document.fonts.ready);
    if (scroll) {
      const target = page.locator(scroll).first();
      if ((await target.count()) > 0) {
        await target.scrollIntoViewIfNeeded();
      }
    }
    await page.waitForTimeout(250);
    const dest = `${REPO}/${file}`;
    await page.screenshot({ path: dest, fullPage: false });
    console.log(`wrote ${dest}`);
  }
  await browser.close();
} finally {
  preview.kill("SIGTERM");
  setTimeout(() => preview.kill("SIGKILL"), 500);
}

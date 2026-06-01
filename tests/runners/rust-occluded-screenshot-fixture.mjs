import fs from "node:fs/promises";
import fsSync from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { callHelper } from "./helper-stdio.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const repoRoot = path.resolve(crateRoot, "..");
const helperPath = path.join(crateRoot, "target/debug/sky.exe");
const fixturePath = path.join(crateRoot, "tests/fixtures/occluded-screenshot-fixture.ps1");

const title = `SkyOccluded-${Date.now()}`;
const statePath = path.join(os.tmpdir(), `sky-occluded-${Date.now()}.json`);

const fixture = spawn(
  "powershell",
  [
    "-NoProfile",
    "-STA",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    fixturePath,
    "-Title",
    title,
    "-StatePath",
    statePath,
  ],
  {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: false,
  },
);

const stderr = [];
fixture.stderr.on("data", (chunk) => stderr.push(chunk));

try {
  await waitForState((state) => state.ready === true, 10000);
  const window = await waitForWindow(title, 10000);
  const state = await callHelper({
    executable: helperPath,
    method: "get_window_state",
    params: {
      window,
      include_screenshot: true,
      include_text: false,
    },
    timeoutMs: 20000,
  });
  if (!state.ok || state.response?.ok !== true) {
    throw new Error(`get_window_state failed: ${JSON.stringify(state)}`);
  }

  const screenshot = state.response.result?.screenshots?.[0];
  if (!screenshot?.url?.startsWith("data:image/png;base64,")) {
    throw new Error(`missing PNG screenshot: ${JSON.stringify(screenshot)}`);
  }

  const color = inspectCenterPixel(screenshot.url);
  const isTargetGreen = color.g > 180 && color.r < 120 && color.b < 120;
  const isOccluderMagenta = color.r > 180 && color.b > 180 && color.g < 120;
  if (!isTargetGreen || isOccluderMagenta) {
    throw new Error(`occluded screenshot did not capture target window center: ${JSON.stringify(color)}`);
  }

  console.log(
    JSON.stringify(
      {
        ok: true,
        title,
        window: {
          app: window.app,
          id: window.id,
          title: window.title,
        },
        screenshot: {
          width: screenshot.width,
          height: screenshot.height,
          centerPixel: color,
        },
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.log(
    JSON.stringify(
      {
        ok: false,
        error: String(error),
        fixtureStderr: Buffer.concat(stderr).toString("utf8"),
        state: await readState().catch(() => null),
      },
      null,
      2,
    ),
  );
  process.exitCode = 1;
} finally {
  fixture.kill();
  await fs.rm(statePath, { force: true }).catch(() => {});
}

async function waitForWindow(expectedTitle, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const call = await callHelper({ executable: helperPath, method: "list_windows" });
    const windows = call.response?.result ?? [];
    const match = windows.find((window) => window.title === expectedTitle);
    if (match) {
      return match;
    }
    await wait(200);
  }
  throw new Error(`fixture window not found: ${expectedTitle}`);
}

async function waitForState(predicate, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const state = await readState().catch(() => null);
    if (state && predicate(state)) {
      return state;
    }
    await wait(100);
  }
  throw new Error("fixture state condition timed out");
}

async function readState() {
  const text = await fs.readFile(statePath, "utf8");
  return JSON.parse(text.replace(/^\uFEFF/, ""));
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function inspectCenterPixel(dataUrl) {
  const base64 = dataUrl.slice("data:image/png;base64,".length);
  const pngPath = path.join(os.tmpdir(), `sky-occluded-shot-${Date.now()}.png`);
  fsSync.writeFileSync(pngPath, Buffer.from(base64, "base64"));
  const script = `
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new('${pngPath.replaceAll("'", "''")}')
$pixel = $bitmap.GetPixel([Math]::Floor($bitmap.Width / 2), [Math]::Floor($bitmap.Height / 2))
[Console]::Out.Write(($pixel.R.ToString() + ',' + $pixel.G.ToString() + ',' + $pixel.B.ToString() + ',' + $pixel.A.ToString()))
$bitmap.Dispose()
`;
  try {
    const result = spawnSync("powershell", ["-NoProfile", "-Command", script], {
      encoding: "utf8",
      windowsHide: true,
      maxBuffer: 1024 * 1024,
    });
    if (result.status !== 0) {
      throw new Error(`failed to inspect screenshot pixel: ${result.stderr || result.stdout}`);
    }
    const [r, g, b, a] = result.stdout.split(",").map((value) => Number(value));
    return { r, g, b, a };
  } finally {
    fsSync.rmSync(pngPath, { force: true });
  }
}


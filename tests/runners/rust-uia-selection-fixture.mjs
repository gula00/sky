import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { callHelper } from "./helper-stdio.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const repoRoot = path.resolve(crateRoot, "..");
const helperPath = path.join(crateRoot, "target/debug/sky.exe");
const fixturePath = path.join(crateRoot, "tests/fixtures/focused-selection-fixture.ps1");

const title = `SkySelectionFixture-${Date.now()}`;
const statePath = path.join(os.tmpdir(), `sky-selection-${Date.now()}.json`);

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
let lastAccessibility = null;
fixture.stderr.on("data", (chunk) => stderr.push(chunk));

try {
  const fixtureState = await waitForState((state) => state.ready === true, 10000);
  const window = await waitForWindow(title, 10000);
  await assertOk(
    callHelper({
      executable: helperPath,
      method: "activate_window",
      params: { window },
    }),
    "activate_window",
  );
  await wait(250);
  const accessibility = await readAccessibility(window);
  lastAccessibility = summarizeAccessibility(accessibility);

  const hasFocusedElement =
    typeof accessibility.focused_element === "string" &&
    accessibility.focused_element.includes("focused=true");
  const hasSelectedText =
    typeof accessibility.selected_text === "string" &&
    accessibility.selected_text.includes(fixtureState.selectedText);
  const hasSelectedElement = accessibility.selected_elements?.some((line) =>
    line.includes(fixtureState.selectedElement) && line.includes("selected=true"),
  );
  const treeContainsTextboxValue = accessibility.tree.includes(fixtureState.textValue);

  if (!hasFocusedElement) {
    throw new Error(`focused_element missing focused=true: ${accessibility.focused_element}`);
  }
  if (!hasSelectedText) {
    throw new Error(`selected_text missing ${fixtureState.selectedText}: ${accessibility.selected_text}`);
  }
  if (!hasSelectedElement) {
    throw new Error(
      `selected_elements missing ${fixtureState.selectedElement}: ${JSON.stringify(
        accessibility.selected_elements,
      )}`,
    );
  }
  if (!treeContainsTextboxValue) {
    throw new Error("accessibility tree is missing textbox value");
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
        fixtureState,
        accessibility: {
          focusedElement: accessibility.focused_element,
          selectedText: accessibility.selected_text,
          selectedElements: accessibility.selected_elements,
          treeLineCount: accessibility.tree.split("\n").filter(Boolean).length,
          treeContainsTextboxValue,
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
        accessibility: lastAccessibility,
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

async function assertOk(callPromise, label) {
  const call = await callPromise;
  if (!call.ok || call.response?.ok !== true) {
    throw new Error(`${label} failed: ${JSON.stringify(call)}`);
  }
}

async function readAccessibility(window) {
  const call = await callHelper({
    executable: helperPath,
    method: "get_window_state",
    params: {
      window,
      include_screenshot: false,
      include_text: true,
    },
  });
  if (!call.ok || call.response?.ok !== true) {
    throw new Error(`get_window_state failed: ${JSON.stringify(call)}`);
  }

  const accessibility = call.response.result?.accessibility;
  if (!accessibility?.tree) {
    throw new Error("fixture accessibility tree is missing");
  }
  return accessibility;
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function summarizeAccessibility(accessibility) {
  return {
    focusedElement: accessibility.focused_element,
    selectedText: accessibility.selected_text,
    selectedElements: accessibility.selected_elements,
    documentText: accessibility.document_text,
    treePreview: accessibility.tree.split("\n").slice(0, 8),
  };
}


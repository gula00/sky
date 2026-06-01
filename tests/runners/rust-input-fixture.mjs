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
const fixturePath = path.join(crateRoot, "tests/fixtures/input-fixture.ps1");

const title = `SkyFixture-${Date.now()}`;
const statePath = path.join(os.tmpdir(), `sky-fixture-${Date.now()}.json`);

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
  const accessibility = await readAccessibility(window);
  assertTreeContains(accessibility.tree, ["Click Target", "Drag Target"]);
  const clickTargetIndex = findTreeIndex(accessibility.tree, "Click Target");
  const inputIndex = findTreeIndex(accessibility.tree, "EDIT");

  await assertOk(
    callHelper({
      executable: helperPath,
      method: "activate_window",
      params: { window },
    }),
    "activate_window",
  );

  await assertOk(
    callHelper({
      executable: helperPath,
      method: "click_element",
      params: { window, element_index: clickTargetIndex, click_count: 1, mouse_button: "left" },
    }),
    "click_element",
  );
  await waitForState((state) => state.clickCount >= 1, 5000);

  await assertOk(
    callHelper({
      executable: helperPath,
      method: "set_value",
      params: { window, element_index: inputIndex, value: "set-by-index" },
    }),
    "set_value",
  );
  await waitForState((state) => state.textValue === "set-by-index", 5000);
  await assertOk(
    callHelper({
      executable: helperPath,
      method: "click",
      params: { window, element_index: inputIndex, click_count: 1, mouse_button: "left" },
    }),
    "click element_index",
  );
  await wait(150);
  await assertOk(
    callHelper({
      executable: helperPath,
      method: "press_key",
      params: { window, key: "Control_L+a" },
    }),
    "press_key",
  );
  await wait(150);
  await assertOk(
    callHelper({
      executable: helperPath,
      method: "type_text",
      params: { window, text: "hello-rust" },
    }),
    "type_text",
  );
  await waitForState((state) => state.textValue === "hello-rust", 5000);

  await assertOk(
    callHelper({
      executable: helperPath,
      method: "click",
      params: { window, x: 110, y: 160, click_count: 1, mouse_button: "left" },
    }),
    "click scroll target",
  );
  await wait(150);
  await assertOk(
    callHelper({
      executable: helperPath,
      method: "scroll",
      params: { window, x: 110, y: 160, scrollX: 0, scrollY: 3 },
    }),
    "scroll",
  );
  await waitForState((state) => state.scrollValue > 0, 5000);

  await assertOk(
    callHelper({
      executable: helperPath,
      method: "drag",
      params: { window, from_x: 255, from_y: 158, to_x: 330, to_y: 180 },
    }),
    "drag",
  );
  await waitForState((state) => state.dragValue === "dragged", 5000);

  const unsupportedSecondary = await callHelper({
    executable: helperPath,
    method: "perform_secondary_action",
    params: { window, element_index: clickTargetIndex, action: "Definitely Unsupported" },
  });
  if (
    !unsupportedSecondary.ok ||
    unsupportedSecondary.response?.ok !== false ||
    !unsupportedSecondary.response?.error?.includes("unsupported secondary action")
  ) {
    throw new Error(
      `unsupported perform_secondary_action did not produce a clear error: ${JSON.stringify(
        unsupportedSecondary,
      )}`,
    );
  }

  const finalState = await readState();
  const result = {
    ok: true,
    title,
    window: {
      app: window.app,
      id: window.id,
      title: window.title,
    },
    finalState,
    accessibility: {
      treeLineCount: accessibility.tree.split("\n").filter(Boolean).length,
      containsClickTarget: accessibility.tree.includes("Click Target"),
      containsDragTarget: accessibility.tree.includes("Drag Target"),
      clickTargetIndex,
      inputIndex,
      unsupportedSecondaryError: unsupportedSecondary.response.error,
    },
  };
  console.log(JSON.stringify(result, null, 2));
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

async function assertOk(callPromise, label) {
  const call = await callPromise;
  if (!call.ok || call.response?.ok !== true) {
    throw new Error(`${label} failed: ${JSON.stringify(call)}`);
  }
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

function assertTreeContains(tree, values) {
  for (const value of values) {
    if (!tree.includes(value)) {
      throw new Error(`fixture accessibility tree is missing ${value}`);
    }
  }
}

function findTreeIndex(tree, needle) {
  const line = tree.split("\n").find((line) => line.includes(needle));
  if (!line) {
    throw new Error(`fixture accessibility tree is missing ${needle}`);
  }
  const match = /^(\d+)/.exec(line);
  if (!match) {
    throw new Error(`fixture accessibility line has no index: ${line}`);
  }
  return Number(match[1]);
}


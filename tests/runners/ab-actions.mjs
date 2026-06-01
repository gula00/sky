import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import readline from "node:readline";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const fixturePath = path.join(crateRoot, "tests/fixtures/input-fixture.ps1");
const originalHelperPath = process.env.SKY_ORIGINAL_HELPER;

if (!originalHelperPath) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        error: "Set SKY_ORIGINAL_HELPER to the original helper executable for A/B tests.",
      },
      null,
      2,
    ),
  );
  process.exit(2);
}

const backends = [
  {
    backend: "original",
    executable: path.resolve(originalHelperPath),
  },
  {
    backend: "rust",
    executable: path.join(crateRoot, "target/debug/sky.exe"),
  },
];

async function runBackendScenario({ backend, executable }) {
  await fs.access(executable);

  const title = `ComputerUseActionAB-${backend}-${Date.now()}`;
  const statePath = path.join(os.tmpdir(), `computer-use-action-ab-${backend}-${Date.now()}.json`);
  const fixtureHost = await createFixtureHost({ renamed: backend === "original" });
  const helper = new HelperSession(executable);
  const fixture = spawn(
    fixtureHost.executable,
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
      cwd: crateRoot,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: false,
    },
  );

  const stderr = [];
  fixture.stderr.on("data", (chunk) => stderr.push(chunk));
  const actionObservations = {};

  try {
    await waitForState(statePath, (state) => state.ready === true, 10000);
    const window = await waitForWindow(helper, title, 10000);
    const points = fixturePoints(backend);

    let { accessibility, clickTargetIndex, inputIndex } = await readFixtureIndexes(helper, window);

    await assertOk(
      callHelper({
        helper,
        method: "click_element",
        params: { window, element_index: clickTargetIndex, click_count: 1, mouse_button: "left" },
      }),
      "click_element button",
    );
    await waitForState(statePath, (state) => state.clickCount >= 1, 5000);

    ({ inputIndex } = await readFixtureIndexes(helper, window));
    if (backend === "original") {
      actionObservations.setValue = {
        ok: false,
        error: "original helper rejects set_value on the WinForms Edit fixture; keyboard fallback used",
      };
      await setTextByKeyboard(helper, window, "set-by-index", points.input);
    } else {
      const setValueCall = await callHelper({
        helper,
        method: "set_value",
        params: { window, element_index: inputIndex, value: "set-by-index" },
      });
      actionObservations.setValue = summarizeCall(setValueCall);
      if (!setValueCall.ok || setValueCall.response?.ok !== true) {
        throw new Error(`set_value failed: ${JSON.stringify(setValueCall)}`);
      }
    }
    await waitForState(statePath, (state) => state.textValue === "set-by-index", 5000);

    ({ inputIndex } = await readFixtureIndexes(helper, window));
    await assertOk(
      callHelper({
        helper,
        method: "click_element",
        params: { window, element_index: inputIndex, click_count: 1, mouse_button: "left" },
      }),
      "click_element input",
    );
    await readAccessibility(helper, window);
    await assertOk(
      callHelper({
        helper,
        method: "click",
        params: { window, ...points.input, click_count: 1, mouse_button: "left" },
      }),
      "click input coordinate",
    );
    await wait(150);
    await assertOk(
      callHelper({
        helper,
        method: "press_key",
        params: { window, key: "Control_L+a" },
      }),
      "press_key",
    );
    await wait(150);
    await assertOk(
      callHelper({
        helper,
        method: "type_text",
        params: { window, text: "hello-rust" },
      }),
      "type_text",
    );
    await waitForState(statePath, (state) => state.textValue === "hello-rust", 5000);

    await readAccessibility(helper, window);
    await assertOk(
      callHelper({
        helper,
        method: "click",
        params: { window, ...points.scroll, click_count: 1, mouse_button: "left" },
      }),
      "click scroll target coordinate",
    );
    await wait(150);
    await assertOk(
      callHelper({
        helper,
        method: "scroll",
        params: { window, ...points.scroll, scrollX: 0, scrollY: 3 },
      }),
      "scroll",
    );
    await waitForState(statePath, (state) => state.scrollValue > 0, 5000);

    await readAccessibility(helper, window);
    await assertOk(
      callHelper({
        helper,
        method: "drag",
        params: { window, ...points.drag },
      }),
      "drag",
    );
    await waitForState(statePath, (state) => state.dragValue === "dragged", 5000);

    ({ clickTargetIndex } = await readFixtureIndexes(helper, window));
    if (backend === "original") {
      actionObservations.performSecondaryAction = {
        ok: false,
        error: "original helper rejects Invoke for the WinForms Button fixture; click fallback used",
      };
      await assertOk(
        callHelper({
          helper,
          method: "click_element",
          params: {
            window,
            element_index: clickTargetIndex,
            click_count: 1,
            mouse_button: "left",
          },
        }),
        "click_element secondary fallback",
      );
    } else {
      const secondaryCall = await callHelper({
        helper,
        method: "perform_secondary_action",
        params: { window, element_index: clickTargetIndex, action: "Invoke" },
      });
      actionObservations.performSecondaryAction = summarizeCall(secondaryCall);
      if (!secondaryCall.ok || secondaryCall.response?.ok !== true) {
        throw new Error(`perform_secondary_action failed: ${JSON.stringify(secondaryCall)}`);
      }
    }
    await waitForState(statePath, (state) => state.clickCount >= 2, 5000);

    return {
      ok: true,
      backend,
      window: {
        app: window.app,
        id: window.id,
        title: window.title,
      },
      finalState: await readState(statePath),
      accessibility: {
        treeLineCount: accessibility.tree.split("\n").filter(Boolean).length,
        clickTargetIndex,
        inputIndex,
      },
      actionObservations,
    };
  } catch (error) {
    return {
      ok: false,
      backend,
      error: String(error),
      fixtureStderr: Buffer.concat(stderr).toString("utf8"),
      state: await readState(statePath).catch(() => null),
    };
  } finally {
    await helper.close();
    fixture.kill();
    await waitForProcessExit(fixture, 3000);
    await fs.rm(statePath, { force: true }).catch(() => {});
    await fixtureHost.cleanup();
  }
}

async function readFixtureIndexes(helper, window) {
  const accessibility = await readAccessibility(helper, window);
  return {
    accessibility,
    clickTargetIndex: findTreeIndex(accessibility.tree, [/Click Target/i]),
    inputIndex: findTreeIndex(accessibility.tree, [/InputText/i, /\bEDIT\b/i, /\bedit\b/i]),
  };
}

async function setTextByKeyboard(helper, window, text, inputPoint) {
  const { inputIndex } = await readFixtureIndexes(helper, window);
  await assertOk(
    callHelper({
      helper,
      method: "click_element",
      params: { window, element_index: inputIndex, click_count: 1, mouse_button: "left" },
    }),
    "click_element input fallback",
  );
  await wait(150);
  await readAccessibility(helper, window);
  await assertOk(
    callHelper({
      helper,
      method: "click",
      params: { window, ...inputPoint, click_count: 1, mouse_button: "left" },
    }),
    "click input coordinate fallback",
  );
  await wait(150);
  await assertOk(
    callHelper({
      helper,
      method: "press_key",
      params: { window, key: "Control_L+a" },
    }),
    "press_key fallback",
  );
  await wait(150);
  await assertOk(
    callHelper({
      helper,
      method: "type_text",
      params: { window, text },
    }),
    "type_text fallback",
  );
}

function fixturePoints(backend) {
  if (backend === "original") {
    return {
      input: { x: 100, y: 55 },
      scroll: { x: 160, y: 240 },
      drag: { from_x: 390, from_y: 250, to_x: 500, to_y: 290 },
    };
  }

  return {
    input: { x: 100, y: 55 },
    scroll: { x: 110, y: 160 },
    drag: { from_x: 255, from_y: 158, to_x: 330, to_y: 180 },
  };
}

function summarizeCall(call) {
  return {
    ok: Boolean(call.ok && call.response?.ok === true),
    error: call.response?.error ?? call.error ?? null,
  };
}

async function assertOk(callPromise, label) {
  const call = await callPromise;
  if (!call.ok || call.response?.ok !== true) {
    throw new Error(`${label} failed: ${JSON.stringify(call)}`);
  }
}

async function waitForWindow(helper, expectedTitle, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const call = await callHelper({ helper, method: "list_windows" });
    const windows = call.response?.result ?? [];
    const match = windows.find((window) => window.title === expectedTitle);
    if (match) {
      return match;
    }
    await wait(200);
  }
  throw new Error(`fixture window not found: ${expectedTitle}`);
}

async function readAccessibility(helper, window) {
  const call = await callHelper({
    helper,
    method: "get_window_state",
    params: {
      window,
      include_screenshot: true,
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

async function waitForState(statePath, predicate, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const state = await readState(statePath).catch(() => null);
    if (state && predicate(state)) {
      return state;
    }
    await wait(100);
  }
  throw new Error("fixture state condition timed out");
}

async function readState(statePath) {
  const text = await fs.readFile(statePath, "utf8");
  return JSON.parse(text.replace(/^\uFEFF/, ""));
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function callHelper({ helper, method, params = {}, timeoutMs = 10000 }) {
  return helper.call({ method, params, timeoutMs });
}

class HelperSession {
  constructor(executable) {
    this.executable = executable;
    this.child = spawn(executable, ["--parent-pid", String(process.pid)], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.lines = readline.createInterface({ input: this.child.stdout });
    this.stderr = [];
    this.nextId = 1;
    this.closed = false;
    this.child.stderr.on("data", (chunk) => this.stderr.push(chunk));
  }

  async call({ method, params = {}, timeoutMs = 10000 }) {
    const request = {
      id: this.nextId++,
      method,
      params,
      meta: { "x-oai-cua-request-budget-ms": timeoutMs },
    };

    try {
      this.child.stdin.write(`${JSON.stringify(request)}\n`);
      let line = await this.readResponseLine({ timeoutMs, label: method });
      let response = JSON.parse(line);

      if (response?.approvalRequest?.app) {
        request.id = this.nextId++;
        request.meta = {
          ...request.meta,
          "x-oai-cua-approved-app": response.approvalRequest.app,
        };
        this.child.stdin.write(`${JSON.stringify(request)}\n`);
        line = await this.readResponseLine({ timeoutMs, label: `${method} approval retry` });
        response = JSON.parse(line);
      }

      return {
        ok: true,
        response,
        stderr: Buffer.concat(this.stderr).toString("utf8"),
      };
    } catch (error) {
      return {
        ok: false,
        error: String(error),
        stderr: Buffer.concat(this.stderr).toString("utf8"),
      };
    }
  }

  readResponseLine({ timeoutMs, label }) {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        cleanup();
        this.child.kill();
        reject(new Error(`helper response timed out: ${label}`));
      }, timeoutMs);

      const onLine = (line) => {
        cleanup();
        resolve(line);
      };
      const onClose = () => {
        cleanup();
        reject(new Error(`helper stdout closed before response: ${label}`));
      };
      const onExit = (code, signal) => {
        cleanup();
        reject(new Error(`helper exited before response: ${label} (${code ?? signal ?? "unknown"})`));
      };
      const onError = (error) => {
        cleanup();
        reject(error);
      };
      const cleanup = () => {
        clearTimeout(timeout);
        this.lines.off("line", onLine);
        this.lines.off("close", onClose);
        this.child.off("exit", onExit);
        this.child.off("error", onError);
      };

      this.lines.once("line", onLine);
      this.lines.once("close", onClose);
      this.child.once("exit", onExit);
      this.child.once("error", onError);
    });
  }

  async close() {
    if (this.closed) {
      return;
    }
    this.closed = true;
    if (!this.child.killed && this.child.stdin.writable) {
      this.child.stdin.write(
        `${JSON.stringify({ id: this.nextId++, method: "close", params: {}, meta: {} })}\n`,
      );
      this.child.stdin.end();
    }
    this.lines.close();
    await waitForProcessExit(this.child, 1000);
    if (!this.child.killed) {
      this.child.kill();
    }
  }
}

async function createFixtureHost({ renamed }) {
  if (!renamed) {
    return {
      executable: "powershell",
      cleanup: async () => {},
    };
  }

  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "computer-use-action-ab-host-"));
  const source = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const executable = path.join(dir, "ComputerUseFixtureHost.exe");
  await fs.copyFile(source, executable);
  return {
    executable,
    cleanup: () => fs.rm(dir, { recursive: true, force: true }).catch(() => {}),
  };
}

function waitForProcessExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    const timer = setTimeout(resolve, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

function findTreeIndex(tree, patterns) {
  const line = tree.split("\n").find((line) => patterns.some((pattern) => pattern.test(line)));
  if (!line) {
    throw new Error(`fixture accessibility tree is missing ${patterns.map(String).join(" or ")}`);
  }
  const match = /^\s*(\d+)/.exec(line);
  if (!match) {
    throw new Error(`fixture accessibility line has no index: ${line}`);
  }
  return Number(match[1]);
}

function comparableState(original, rust) {
  return (
    original?.clickCount >= 2 &&
    rust?.clickCount >= 2 &&
    original?.textValue === "hello-rust" &&
    rust?.textValue === "hello-rust" &&
    original?.scrollValue > 0 &&
    rust?.scrollValue > 0 &&
    original?.dragValue === "dragged" &&
    rust?.dragValue === "dragged"
  );
}

async function main() {
  const results = [];
  for (const backend of backends) {
    results.push(await runBackendScenario(backend));
  }

  const ok =
    results.every((result) => result.ok) &&
    comparableState(results[0].finalState, results[1].finalState);

  console.log(JSON.stringify({ ok, results }, null, 2));
  if (!ok) {
    process.exitCode = 1;
  }
}

await main();


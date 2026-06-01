import { spawn } from "node:child_process";
import readline from "node:readline";

export async function callHelper({
  executable,
  method,
  params = {},
  meta = {},
  timeoutMs = 10000,
}) {
  const child = spawn(executable, ["--parent-pid", String(process.pid)], {
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });

  const stderr = [];
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  const lines = readline.createInterface({ input: child.stdout });
  const request = {
    id: 1,
    method,
    params,
    meta: { ...meta, "x-oai-cua-request-budget-ms": timeoutMs },
  };

  const timer = setTimeout(() => {
    child.kill();
  }, timeoutMs + 1000);

  try {
    child.stdin.write(`${JSON.stringify(request)}\n`);
    let line = await readResponseLine({ child, lines, timeoutMs, label: method });
    let response = JSON.parse(line);

    if (response?.approvalRequest?.app) {
      request.id = 2;
      request.meta = {
        ...request.meta,
        "x-oai-cua-approved-app": response.approvalRequest.app,
      };
      child.stdin.write(`${JSON.stringify(request)}\n`);
      line = await readResponseLine({ child, lines, timeoutMs, label: `${method} approval retry` });
      response = JSON.parse(line);
    }

    child.stdin.write(
      `${JSON.stringify({ id: 9999, method: "close", params: {}, meta: {} })}\n`,
    );
    child.stdin.end();

    return {
      ok: true,
      response,
      stderr: Buffer.concat(stderr).toString("utf8"),
    };
  } catch (error) {
    return {
      ok: false,
      error: String(error),
      stderr: Buffer.concat(stderr).toString("utf8"),
    };
  } finally {
    clearTimeout(timer);
    lines.close();
    if (!child.killed) {
      child.kill();
    }
  }
}

function readResponseLine({ child, lines, timeoutMs, label }) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cleanup();
      child.kill();
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
      lines.off("line", onLine);
      lines.off("close", onClose);
      child.off("exit", onExit);
      child.off("error", onError);
    };

    lines.once("line", onLine);
    lines.once("close", onClose);
    child.once("exit", onExit);
    child.once("error", onError);
  });
}

export async function runHelperCase({ backend, executable, testCase }) {
  const params = await resolveParams({ executable, testCase });
  const call = await callHelper({
    executable,
    method: testCase.method,
    params,
  });
  return normalizeObservation({ backend, testCase, call });
}

async function resolveParams({ executable, testCase }) {
  if (testCase.paramsFrom === "firstWindow") {
    const listed = await callHelper({ executable, method: "list_windows" });
    const firstWindow = listed.response?.result?.[0];
    if (!firstWindow) {
      throw new Error("cannot resolve firstWindow params");
    }
    return firstWindow;
  }

  if (testCase.paramsFrom === "firstWindowStateText") {
    return findUsableWindowStateParams({
      executable,
      include_screenshot: false,
      include_text: true,
      label: "firstWindowStateText",
    });
  }

  if (testCase.paramsFrom === "firstWindowStateScreenshot") {
    return findUsableWindowStateParams({
      executable,
      include_screenshot: true,
      include_text: false,
      label: "firstWindowStateScreenshot",
    });
  }

  return testCase.params ?? {};
}

async function findUsableWindowStateParams({
  executable,
  include_screenshot,
  include_text,
  label,
}) {
  const listed = await callHelper({ executable, method: "list_windows" });
  const windows = listed.response?.result ?? [];
  const errors = [];

  for (const window of windows.slice(0, 12)) {
    const params = {
      window,
      include_screenshot,
      include_text,
    };
    const probe = await callHelper({
      executable,
      method: "get_window_state",
      params,
      timeoutMs: include_screenshot ? 20000 : 10000,
    });
    if (probe.ok && probe.response?.ok === true) {
      return params;
    }
    errors.push({
      title: window.title,
      error: probe.response?.error ?? probe.error ?? "unknown get_window_state failure",
    });
  }

  throw new Error(`cannot resolve ${label} params: ${JSON.stringify(errors)}`);
}

export function normalizeObservation({ backend, testCase, call }) {
  const response = call.response;
  const result = response?.result;
  const isArray = Array.isArray(result);
  const isObject = result !== null && typeof result === "object" && !isArray;
  const first = isArray && result.length > 0 ? result[0] : null;
  const screenshot = isObject ? result.screenshots?.[0] : null;
  const screenshotDecoded = decodeScreenshot(screenshot);

  return {
    case: testCase.id,
    backend,
    ok: Boolean(call.ok && response?.ok === true),
    observations: {
      method: testCase.method,
      resultIsArray: isArray,
      resultIsObject: isObject,
      resultCount: isArray ? result.length : null,
      resultKeys: isObject ? Object.keys(result).sort() : [],
      firstItemKeys: first && typeof first === "object" ? Object.keys(first).sort() : [],
      screenshot: screenshot
        ? {
            keys: Object.keys(screenshot).sort(),
            width: screenshot.width ?? null,
            height: screenshot.height ?? null,
            mime: screenshotDecoded.mime,
            decodable: screenshotDecoded.decodable,
            bytes: screenshotDecoded.bytes,
          }
        : null,
    },
    errors: collectErrors(call, response, testCase),
  };
}

function collectErrors(call, response, testCase) {
  const errors = [];
  if (!call.ok) {
    errors.push(call.error ?? "helper call failed");
  }
  if (response?.ok !== true) {
    errors.push(response?.error ?? "helper returned non-ok response");
  }
  if (testCase.expectArray && !Array.isArray(response?.result)) {
    errors.push("result is not an array");
  }
  if (testCase.expectObjectKeys) {
    const result = response?.result;
    const keys =
      result !== null && typeof result === "object" && !Array.isArray(result)
        ? Object.keys(result)
        : [];
    for (const key of testCase.expectObjectKeys) {
      if (!keys.includes(key)) {
        errors.push(`result is missing key: ${key}`);
      }
    }
  }
  if (testCase.expectScreenshot) {
    const screenshot = response?.result?.screenshots?.[0];
    const decoded = decodeScreenshot(screenshot);
    if (!screenshot) {
      errors.push("missing screenshot");
    }
    if (!decoded.decodable) {
      errors.push("screenshot is not decodable");
    }
    if (!(screenshot?.width > 0) || !(screenshot?.height > 0)) {
      errors.push("screenshot dimensions are invalid");
    }
  }
  return errors;
}

function decodeScreenshot(screenshot) {
  const url = screenshot?.url;
  if (typeof url !== "string") {
    return { decodable: false, mime: null, bytes: 0 };
  }

  const match = /^data:([^;]+);base64,(.*)$/s.exec(url);
  if (!match) {
    return { decodable: false, mime: null, bytes: 0 };
  }

  try {
    const bytes = Buffer.from(match[2], "base64");
    return {
      decodable: bytes.length > 0,
      mime: match[1],
      bytes: bytes.length,
    };
  } catch {
    return { decodable: false, mime: match[1], bytes: 0 };
  }
}

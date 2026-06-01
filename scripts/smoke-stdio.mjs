import path from "node:path";
import { fileURLToPath } from "node:url";
import { callHelper } from "../tests/runners/helper-stdio.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "..");
const executable = process.argv[2] ?? path.join(crateRoot, "target/debug/sky.exe");

const diagnostic = await callHelper({
  executable,
  method: "diagnostic_state",
});
const windows = await callHelper({
  executable,
  method: "list_windows",
});
const apps = await callHelper({
  executable,
  method: "list_apps",
});

const result = {
  ok:
    diagnostic.ok &&
    diagnostic.response?.ok === true &&
    windows.ok &&
    windows.response?.ok === true &&
    apps.ok &&
    apps.response?.ok === true,
  executable,
  diagnostic: diagnostic.response?.result ?? null,
  windows: {
    count: windows.response?.result?.length ?? null,
    firstKeys: Object.keys(windows.response?.result?.[0] ?? {}).sort(),
  },
  apps: {
    count: apps.response?.result?.length ?? null,
    firstKeys: Object.keys(apps.response?.result?.[0] ?? {}).sort(),
  },
  errors: [diagnostic, windows, apps]
    .map((call) => call.response?.error ?? call.error)
    .filter(Boolean),
};

console.log(JSON.stringify(result, null, 2));
if (!result.ok) {
  process.exitCode = 1;
}


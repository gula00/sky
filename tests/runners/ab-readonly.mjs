import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runOriginal } from "./original-sky-runner.mjs";
import { runRust } from "./rust-helper-runner.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const casesDir = path.resolve(here, "../cases");

if (!process.env.SKY_ORIGINAL_HELPER) {
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

const caseFiles = (await fs.readdir(casesDir))
  .filter((name) => name.endsWith(".json"))
  .sort();

const results = [];
for (const caseFile of caseFiles) {
  const testCase = JSON.parse(await fs.readFile(path.join(casesDir, caseFile), "utf8"));
  const original = await runOriginal(testCase);
  const rust = await runRust(testCase);
  results.push({
    case: testCase.id,
    pass: compareReadOnly(original, rust),
    original,
    rust,
  });
}

const ok = results.every((result) => result.pass);
console.log(JSON.stringify({ ok, results }, null, 2));
if (!ok) {
  process.exitCode = 1;
}

function compareReadOnly(original, rust) {
  if (!original.ok || !rust.ok) {
    return false;
  }
  if (original.observations.resultIsObject || rust.observations.resultIsObject) {
    return original.errors.length === 0 && rust.errors.length === 0;
  }
  return (
    original.observations.resultIsArray === true &&
    rust.observations.resultIsArray === true
  );
}

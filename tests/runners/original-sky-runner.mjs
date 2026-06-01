import fs from "node:fs/promises";
import path from "node:path";
import { runHelperCase } from "./helper-stdio.mjs";

const helperPath = process.env.SKY_ORIGINAL_HELPER;

export async function runOriginal(testCase) {
  if (!helperPath) {
    throw new Error("Set SKY_ORIGINAL_HELPER to the original helper executable for A/B tests.");
  }
  const executable = path.resolve(helperPath);
  await fs.access(executable);
  return runHelperCase({
    backend: "original",
    executable,
    testCase,
  });
}

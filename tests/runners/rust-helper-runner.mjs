import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runHelperCase } from "./helper-stdio.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const helperPath = path.join(crateRoot, "target/debug/sky.exe");

export async function runRust(testCase) {
  await fs.access(helperPath);
  return runHelperCase({
    backend: "rust",
    executable: helperPath,
    testCase,
  });
}


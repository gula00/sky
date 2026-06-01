import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { callHelper } from "./helper-stdio.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const helperPath = path.join(crateRoot, "target/debug/sky.exe");

const codexHome = path.join(os.tmpdir(), `sky-turn-${Date.now()}`);
const sessionId = "session/with/slash";
const turnId = "turn:with:colon";

try {
  const turnEnded = spawnSync(
    helperPath,
    [
      "turn-ended",
      "--codex-home",
      codexHome,
      "--session-id",
      sessionId,
      "--turn-id",
      turnId,
    ],
    { encoding: "utf8", windowsHide: true },
  );
  if (turnEnded.status !== 0) {
    throw new Error(`turn-ended failed: ${turnEnded.stderr || turnEnded.stdout}`);
  }

  const interruptPath = path.join(
    codexHome,
    "cache",
    "computer-use",
    "interrupts",
    "session_with_slash",
    "turn_with_colon",
  );
  await fs.access(interruptPath);

  const interrupted = await callHelper({
    executable: helperPath,
    method: "list_windows",
    meta: {
      codexHome,
      session_id: sessionId,
      turn_id: turnId,
    },
  });
  if (
    !interrupted.ok ||
    interrupted.response?.ok !== false ||
    !interrupted.response?.error?.includes("Computer Use was stopped by the user")
  ) {
    throw new Error(`interrupted request was not rejected: ${JSON.stringify(interrupted)}`);
  }

  const endTurn = await callHelper({
    executable: helperPath,
    method: "end_turn",
    meta: {
      codexHome,
      session_id: sessionId,
      turn_id: turnId,
    },
  });
  if (!endTurn.ok || endTurn.response?.ok !== true) {
    throw new Error(`end_turn did not bypass interrupted turn: ${JSON.stringify(endTurn)}`);
  }

  console.log(
    JSON.stringify(
      {
        ok: true,
        interruptPath,
        interruptedError: interrupted.response.error,
        endTurnOk: endTurn.response.ok,
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
      },
      null,
      2,
    ),
  );
  process.exitCode = 1;
} finally {
  await fs.rm(codexHome, { recursive: true, force: true }).catch(() => {});
}


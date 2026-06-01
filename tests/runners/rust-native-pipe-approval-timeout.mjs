import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import net from "node:net";
import { endianness } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.log(JSON.stringify({ ok: true, skipped: "native pipe is Windows-only" }, null, 2));
  process.exit(0);
}

const here = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(here, "../..");
const helperPath = path.join(crateRoot, "target/debug/sky.exe");
const pipePath = `\\\\.\\pipe\\sky-approval-timeout-${randomUUID()}`;

async function main() {
  const helper = spawn(helperPath, ["--native-pipe", pipePath], {
    cwd: crateRoot,
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: true,
  });

  const stderr = [];
  helper.stderr.on("data", (chunk) => stderr.push(chunk));

  try {
    const socket = await connectWithRetry(pipePath, 5000);
    const peer = new FramedPeer(socket);
    let approvalCallbacks = 0;

    peer.onRequest = (message) => {
      if (message.method === "requestComputerUseApproval") {
        approvalCallbacks += 1;
        return;
      }

      peer.send({
        error: { code: -32000, message: `unsupported callback: ${message.method}` },
        id: message.id,
        jsonrpc: "2.0",
      });
    };

    const windows = await peer.request("request", {
      codexTurnMetadata: {
        session_id: "rust_native_pipe_approval_timeout",
        turn_id: "turn_1",
      },
      method: "list_windows",
      params: {},
    });
    if (!Array.isArray(windows) || windows.length === 0) {
      throw new Error("list_windows returned no windows to approve against");
    }

    const started = Date.now();
    const error = await peer
      .request(
        "request",
        {
          codexTurnMetadata: {
            session_id: "rust_native_pipe_approval_timeout",
            turn_id: "turn_1",
            "x-oai-cua-request-budget-ms": 300,
          },
          method: "get_window_state",
          params: {
            include_screenshot: false,
            include_text: false,
            window: windows[0],
          },
        },
        { timeoutMs: 5000 },
      )
      .then(
        () => {
          throw new Error("approval timeout request unexpectedly succeeded");
        },
        (error) => error.message,
      );
    const elapsedMs = Date.now() - started;

    if (approvalCallbacks !== 1) {
      throw new Error(`expected one approval callback, got ${approvalCallbacks}`);
    }
    if (!error.includes("timed out")) {
      throw new Error(`approval timeout did not produce timeout error: ${error}`);
    }
    if (elapsedMs > 3000) {
      throw new Error(`approval timeout took too long: ${elapsedMs}ms`);
    }

    await peer.request("close", {});
    socket.destroy();

    console.log(
      JSON.stringify(
        {
          ok: true,
          approvalCallbacks,
          elapsedMs,
          error,
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
          stderr: Buffer.concat(stderr).toString("utf8"),
        },
        null,
        2,
      ),
    );
    process.exitCode = 1;
  } finally {
    helper.kill();
  }
}

function connectWithRetry(pipePath, timeoutMs) {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.createConnection(pipePath);
      socket.once("connect", () => resolve(socket));
      socket.once("error", (error) => {
        socket.destroy();
        if (Date.now() - started >= timeoutMs) {
          reject(error);
          return;
        }
        setTimeout(attempt, 50);
      });
    };
    attempt();
  });
}

class FramedPeer {
  constructor(socket) {
    this.nextId = 1;
    this.onRequest = null;
    this.pending = new Map();
    this.pendingData = Buffer.alloc(0);
    this.socket = socket;
    socket.on("data", (chunk) => this.handleData(chunk));
    socket.on("error", (error) => this.rejectAll(error));
    socket.on("close", () => this.rejectAll(new Error("native pipe closed")));
  }

  request(method, params, { timeoutMs = 10000 } = {}) {
    const id = this.nextId++;
    this.send({ id, jsonrpc: "2.0", method, params });
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`native pipe request timed out: ${method}`));
      }, timeoutMs);
      this.pending.set(id, {
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
      });
    });
  }

  send(message) {
    this.socket.write(encodeFrame(JSON.stringify(message)));
  }

  handleData(chunk) {
    this.pendingData = Buffer.concat([this.pendingData, chunk]);
    const decoded = decodeFrames(this.pendingData);
    this.pendingData = decoded.remaining;
    for (const payload of decoded.messages) {
      this.handleMessage(JSON.parse(payload));
    }
  }

  handleMessage(message) {
    if (message.method && message.id != null) {
      this.onRequest?.(message);
      return;
    }

    const pending = this.pending.get(message.id);
    if (!pending) {
      return;
    }
    this.pending.delete(message.id);

    if (message.error) {
      pending.reject(new Error(message.error.message));
    } else {
      pending.resolve(message.result);
    }
  }

  rejectAll(error) {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }
}

function encodeFrame(message) {
  const payload = Buffer.from(message, "utf8");
  const frame = Buffer.alloc(4 + payload.length);
  if (endianness() === "LE") {
    frame.writeUInt32LE(payload.length, 0);
  } else {
    frame.writeUInt32BE(payload.length, 0);
  }
  payload.copy(frame, 4);
  return frame;
}

function decodeFrames(buffer) {
  const messages = [];
  let offset = 0;
  while (buffer.length - offset >= 4) {
    const length =
      endianness() === "LE" ? buffer.readUInt32LE(offset) : buffer.readUInt32BE(offset);
    if (buffer.length - offset < 4 + length) {
      break;
    }
    messages.push(buffer.subarray(offset + 4, offset + 4 + length).toString("utf8"));
    offset += 4 + length;
  }
  return { messages, remaining: buffer.subarray(offset) };
}

await main();


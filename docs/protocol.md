# Protocol

The helper supports three wire modes.

## Standalone Stdio

Default mode reads newline-delimited JSON requests from stdin and writes one
newline-delimited JSON response per request.

Start:

```text
sky.exe --parent-pid <pid>
```

Request:

```json
{
  "id": 1,
  "method": "list_windows",
  "params": {},
  "meta": {
    "x-oai-cua-request-budget-ms": 10000
  }
}
```

Success:

```json
{
  "id": 1,
  "ok": true,
  "result": []
}
```

Error:

```json
{
  "id": 1,
  "ok": false,
  "error": "unsupported method: example"
}
```

Approval request:

```json
{
  "id": 1,
  "ok": false,
  "approvalRequest": {
    "app": "process:C:\\Path\\App.exe",
    "displayName": "App",
    "riskLevel": "low"
  }
}
```

Approved retry:

```json
{
  "id": 2,
  "method": "get_window_state",
  "params": {
    "window": {
      "app": "process:C:\\Path\\App.exe",
      "id": 123
    }
  },
  "meta": {
    "x-oai-cua-approved-app": "process:C:\\Path\\App.exe",
    "x-oai-cua-request-budget-ms": 10000
  }
}
```

## Framed Stdio

`--frame-stdio` uses 4-byte native-endian length-prefixed JSON-RPC frames over
stdin/stdout.

Start:

```text
sky.exe --frame-stdio
```

The JSON-RPC request shape is:

```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "method": "request",
  "params": {
    "method": "list_windows",
    "params": {},
    "codexTurnMetadata": {}
  }
}
```

The response shape is:

```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": []
}
```

## Native Pipe

`--native-pipe <path>` accepts one Windows named-pipe client and speaks the same
length-prefixed JSON-RPC frames as framed stdio.

Start:

```text
sky.exe --native-pipe \\.\pipe\sky
```

During approval, the helper sends a JSON-RPC callback:

```json
{
  "id": "computer-use-approval:1",
  "jsonrpc": "2.0",
  "method": "requestComputerUseApproval",
  "params": {
    "app": "process:C:\\Path\\App.exe",
    "displayName": "App",
    "riskLevel": "low"
  }
}
```

The peer responds on the same pipe:

```json
{
  "id": "computer-use-approval:1",
  "jsonrpc": "2.0",
  "result": {
    "approved": true
  }
}
```

While an approval is pending, overlapping requests are rejected except `close`
and `end_turn`, which remain available for cleanup.

## Turn Interruption

`turn-ended` writes the turn interruption marker used by request handling:

```text
sky.exe turn-ended --codex-home <dir> --session-id <session> --turn-id <turn>
```

Subsequent requests with matching turn metadata are rejected with the stopped by
user message. `end_turn` and `close` can still pass through.


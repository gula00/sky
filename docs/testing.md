# Testing

The project has three test layers.

## Rust Unit Tests

```powershell
cargo check
cargo test
cargo build
```

These tests cover protocol framing, JSON-RPC routing, approvals, request
budgets, turn interruption, input parsing, and policy helpers.

## Rust Helper Fixture Tests

These tests spawn temporary WinForms fixture windows and drive the Rust helper
against them. They require Windows, PowerShell, Node.js, and an interactive
desktop session.

```powershell
node .\tests\runners\rust-input-fixture.mjs
node .\tests\runners\rust-uia-selection-fixture.mjs
node .\tests\runners\rust-occluded-screenshot-fixture.mjs
node .\tests\runners\rust-turn-interrupt.mjs
node .\tests\runners\rust-native-pipe-approval.mjs
node .\tests\runners\rust-native-pipe-approval-timeout.mjs
```

Or run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures
```

## Original-vs-Rust A/B Tests

A/B tests compare required schema fields and task outcomes. They intentionally
do not compare exact window counts, exact accessibility tree text, or screenshot
bytes.

Set `SKY_ORIGINAL_HELPER` to a compatible original helper executable:

```powershell
$env:SKY_ORIGINAL_HELPER = "C:\path\to\codex-computer-use.exe"
```

Run:

```powershell
node .\tests\runners\ab-readonly.mjs
node .\tests\runners\ab-actions.mjs
```

Or:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures -AB
```

`ab-actions.mjs` records original-helper fixture limitations separately from
Rust failures. For example, an original helper may reject `set_value` or
`Invoke` on the WinForms fixture while Rust succeeds; the runner then uses a
fallback only to keep final task state comparable.

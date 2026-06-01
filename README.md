# sky

`sky` is a Sky-compatible Windows Computer Use helper written in
Rust. It exposes the Window2-style helper protocol used by model-facing desktop
automation clients:

- newline-delimited JSON over stdio for standalone helper mode
- length-prefixed JSON-RPC over stdio for protocol smoke tests
- length-prefixed JSON-RPC over a Windows named pipe for native host mode

The project focuses on Windows window discovery, screenshots, UI Automation
trees, input actions, app approval callbacks, and turn interruption handling.

## Status

This is an independent Rust implementation of the observed Sky Window2 helper
surface. It is intended to be compatible at the API and task-outcome level, not
byte-for-byte identical to any existing helper binary.

Implemented:

- `list_windows`, `list_apps`, `get_window`
- `get_window_state` with screenshot and accessibility state
- Windows.Graphics.Capture screenshots with GDI fallback
- `activate_window`, coordinate `click`, `scroll`, `drag`
- `press_key`, `type_text`
- element-indexed `click`, `click_element`, `set_value`
- UIA `Invoke`, `Value`, `SelectionItem`, `ExpandCollapse`, `ScrollItem`
- standalone and native-pipe approval flows
- turn interruption files and request-budget timeouts
- basic forbidden-target filtering with an explicit test override

Known limits:

- Windows only.
- Original helper output is not reproduced byte-for-byte.
- Physical multi-monitor WGC coordinate behavior needs real hardware coverage.
- Some original-helper filters and transient UI behaviors are intentionally
  treated as outcome-compatible rather than exact clones.

## Layout

```text
bin/                    release binary notes
docs/                   public protocol and API documentation
scripts/                check, smoke, and packaging scripts
src/                    Rust helper implementation
tests/cases/            read-only A/B case definitions
tests/fixtures/         WinForms fixture apps used by runners
tests/runners/          Node.js stdio/native-pipe test runners
```

## Build

```powershell
cargo build
cargo build --release
```

The release packaging script builds the optimized helper and writes a small
manifest:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-release.ps1
```

Generated files go under `dist/`, which is ignored by Git.

## Test

Fast Rust checks:

```powershell
cargo check
cargo test
cargo build
```

Repository check script:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

Fixture tests exercise real Windows UI and require an interactive desktop:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures
```

Original-vs-Rust A/B tests require a compatible original helper binary. Point
`SKY_ORIGINAL_HELPER` at that executable:

```powershell
$env:SKY_ORIGINAL_HELPER = "C:\path\to\codex-computer-use.exe"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures -AB
```

You can also call the runners directly:

```powershell
node .\tests\runners\ab-readonly.mjs
node .\tests\runners\ab-actions.mjs
```

## Smoke Test

After building the debug helper:

```powershell
cargo build
node .\scripts\smoke-stdio.mjs
```

The smoke script starts `target/debug/sky.exe`, calls
`diagnostic_state`, `list_windows`, and `list_apps`, then prints normalized JSON.

## Protocols

See [docs/protocol.md](docs/protocol.md) for wire formats and approval callback
examples.

See [docs/sky-window2-api.md](docs/sky-window2-api.md) for the supported
Window2 method surface.

See [docs/testing.md](docs/testing.md) for the local and A/B validation matrix.

## Safety

The helper requests approval before window/app actions in the observed protocol
flow. It also filters common credential, UAC, logon, and Windows Security
surfaces by default.

For dedicated tests only, forbidden-target filtering can be bypassed with:

```powershell
$env:ComputerUseAllowForbiddenTargets = "true"
```

Do not enable this in normal use.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.


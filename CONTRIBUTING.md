# Contributing

Thanks for improving `sky`.

## Development

Use a recent stable Rust toolchain on Windows.

```powershell
cargo check
cargo test
cargo build
```

Run the full local fixture suite from an interactive Windows desktop:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures
```

If you have a compatible original helper binary, run A/B coverage as well:

```powershell
$env:SKY_ORIGINAL_HELPER = "C:\path\to\codex-computer-use.exe"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1 -Fixtures -AB
```

## Style

- Keep public protocol behavior documented in `docs/`.
- Prefer focused tests that compare task outcomes.
- Avoid committing generated files from `target/` or `dist/`.
- Keep Windows-specific code behind the existing platform gates where possible.

## Pull Requests

Include:

- What changed.
- Which helper methods are affected.
- Which tests were run.
- Any known parity differences from the original helper behavior.


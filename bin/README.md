# Binaries

Release binaries are generated, not checked in.

Build and package the Windows helper with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-release.ps1
```

The packaged executable is written to:

```text
dist/sky.exe
```

The manifest beside it records the package version, executable name, supported
wire protocols, and build timestamp.


param(
  [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"

$crateRoot = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $crateRoot $OutputDirectory
$releaseExe = Join-Path $crateRoot "target\release\sky.exe"
$packagedExe = Join-Path $dist "sky.exe"
$manifestPath = Join-Path $dist "sky.manifest.json"

Push-Location $crateRoot
try {
  cargo build --release
} finally {
  Pop-Location
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination $packagedExe -Force

$manifest = [ordered]@{
  name = "sky"
  version = (Select-String -LiteralPath (Join-Path $crateRoot "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
  executable = "sky.exe"
  protocols = @("stdio-newline-json", "frame-stdio", "native-pipe")
  builtAt = [DateTime]::UtcNow.ToString("o")
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding UTF8

Write-Output "Packaged $packagedExe"


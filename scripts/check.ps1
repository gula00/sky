param(
  [switch]$Fixtures,
  [switch]$AB,
  [switch]$Package,
  [switch]$Release,
  [string]$OriginalHelperPath = $env:SKY_ORIGINAL_HELPER
)

$ErrorActionPreference = "Stop"

$crateRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
  param(
    [string]$Name,
    [scriptblock]$Command
  )

  Write-Host "==> $Name"
  & $Command
}

Push-Location $crateRoot
try {
  Invoke-Step "cargo check" { cargo check }
  Invoke-Step "cargo test" { cargo test }
  Invoke-Step "cargo build" { cargo build }

  if ($Release) {
    Invoke-Step "cargo build --release" { cargo build --release }
  }

  if ($Fixtures -or $AB) {
    Invoke-Step "rust native-pipe approval" { node .\tests\runners\rust-native-pipe-approval.mjs }
    Invoke-Step "rust native-pipe approval timeout" { node .\tests\runners\rust-native-pipe-approval-timeout.mjs }
    Invoke-Step "rust input fixture" { node .\tests\runners\rust-input-fixture.mjs }
    Invoke-Step "rust UIA selection fixture" { node .\tests\runners\rust-uia-selection-fixture.mjs }
    Invoke-Step "rust turn interrupt" { node .\tests\runners\rust-turn-interrupt.mjs }
    Invoke-Step "rust occluded screenshot fixture" { node .\tests\runners\rust-occluded-screenshot-fixture.mjs }
  }

  if ($AB) {
    if ([string]::IsNullOrWhiteSpace($OriginalHelperPath)) {
      throw "A/B tests require SKY_ORIGINAL_HELPER or -OriginalHelperPath."
    }

    $resolvedOriginal = (Resolve-Path -LiteralPath $OriginalHelperPath).Path
    $previousOriginal = $env:SKY_ORIGINAL_HELPER
    $env:SKY_ORIGINAL_HELPER = $resolvedOriginal
    try {
      Invoke-Step "read-only A/B" { node .\tests\runners\ab-readonly.mjs }
      Invoke-Step "action A/B" { node .\tests\runners\ab-actions.mjs }
    } finally {
      $env:SKY_ORIGINAL_HELPER = $previousOriginal
    }
  }

  if ($Package) {
    Invoke-Step "package release" {
      powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-release.ps1
    }
  }
} finally {
  Pop-Location
}

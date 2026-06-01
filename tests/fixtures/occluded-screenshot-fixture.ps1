param(
  [Parameter(Mandatory = $true)]
  [string]$Title,

  [Parameter(Mandatory = $true)]
  [string]$StatePath
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = "Stop"

function Write-State {
  param([bool]$Ready)

  $state = [ordered]@{
    ready = $Ready
    updatedAt = [DateTime]::UtcNow.ToString("o")
  }
  $json = $state | ConvertTo-Json -Compress
  Set-Content -LiteralPath $StatePath -Value $json -Encoding UTF8
}

$target = New-Object System.Windows.Forms.Form
$target.Text = $Title
$target.StartPosition = "Manual"
$target.Location = New-Object System.Drawing.Point(120, 120)
$target.ClientSize = New-Object System.Drawing.Size(360, 240)
$target.BackColor = [System.Drawing.Color]::Lime

$label = New-Object System.Windows.Forms.Label
$label.Text = "WGC Target"
$label.AutoSize = $false
$label.Dock = [System.Windows.Forms.DockStyle]::Fill
$label.TextAlign = [System.Drawing.ContentAlignment]::MiddleCenter
$label.Font = New-Object System.Drawing.Font("Segoe UI", 28, [System.Drawing.FontStyle]::Bold)
$label.ForeColor = [System.Drawing.Color]::Black
$target.Controls.Add($label)

$occluder = New-Object System.Windows.Forms.Form
$occluder.Text = "$Title-Occluder"
$occluder.StartPosition = "Manual"
$occluder.Location = New-Object System.Drawing.Point(120, 120)
$occluder.ClientSize = New-Object System.Drawing.Size(360, 240)
$occluder.BackColor = [System.Drawing.Color]::Magenta
$occluder.TopMost = $true

$target.Add_Shown({
  $occluder.Show()
  $occluder.Activate()
  Write-State -Ready $true
})

[System.Windows.Forms.Application]::EnableVisualStyles()
[System.Windows.Forms.Application]::Run($target)

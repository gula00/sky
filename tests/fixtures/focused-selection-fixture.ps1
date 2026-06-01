param(
  [Parameter(Mandatory = $true)]
  [string]$Title,

  [Parameter(Mandatory = $true)]
  [string]$StatePath
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = "Stop"

$textValue = "alpha selected omega"
$selectionStart = 6
$selectionLength = 8

function Write-State {
  param(
    [bool]$Ready
  )

  $state = [ordered]@{
    ready = $Ready
    textValue = $textValue
    selectionStart = $selectionStart
    selectionLength = $selectionLength
    selectedText = $textValue.Substring($selectionStart, $selectionLength)
    selectedElement = "Option Beta"
    updatedAt = [DateTime]::UtcNow.ToString("o")
  }
  $json = $state | ConvertTo-Json -Compress
  Set-Content -LiteralPath $StatePath -Value $json -Encoding UTF8
}

$form = New-Object System.Windows.Forms.Form
$form.Text = $Title
$form.StartPosition = "Manual"
$form.Location = New-Object System.Drawing.Point(120, 120)
$form.ClientSize = New-Object System.Drawing.Size(420, 260)
$form.TopMost = $true

$textBox = New-Object System.Windows.Forms.TextBox
$textBox.Name = "SelectionInput"
$textBox.Text = $textValue
$textBox.Location = New-Object System.Drawing.Point(24, 24)
$textBox.Size = New-Object System.Drawing.Size(300, 28)

$label = New-Object System.Windows.Forms.Label
$label.Name = "SelectionLabel"
$label.Text = "Focused Selection Fixture"
$label.Location = New-Object System.Drawing.Point(24, 72)
$label.Size = New-Object System.Drawing.Size(260, 28)

$listBox = New-Object System.Windows.Forms.ListBox
$listBox.Name = "SelectionList"
$listBox.Location = New-Object System.Drawing.Point(24, 112)
$listBox.Size = New-Object System.Drawing.Size(220, 80)
[void]$listBox.Items.Add("Option Alpha")
[void]$listBox.Items.Add("Option Beta")
[void]$listBox.Items.Add("Option Gamma")
$listBox.SelectedIndex = 1

$form.Controls.Add($textBox)
$form.Controls.Add($label)
$form.Controls.Add($listBox)

$form.Add_Shown({
  $form.Activate()
  $textBox.Focus()
  $textBox.Select($selectionStart, $selectionLength)
  Write-State -Ready $true
})

$form.Add_Activated({
  $textBox.Focus()
  $textBox.Select($selectionStart, $selectionLength)
  Write-State -Ready $true
})

[System.Windows.Forms.Application]::EnableVisualStyles()
[System.Windows.Forms.Application]::Run($form)

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
  param(
    [int]$ClickCount,
    [string]$TextValue,
    [int]$ScrollValue,
    [string]$DragValue,
    [bool]$Ready
  )

  $state = [ordered]@{
    ready = $Ready
    clickCount = $ClickCount
    textValue = $TextValue
    scrollValue = $ScrollValue
    dragValue = $DragValue
    updatedAt = [DateTime]::UtcNow.ToString("o")
  }
  $json = $state | ConvertTo-Json -Compress
  Set-Content -LiteralPath $StatePath -Value $json -Encoding UTF8
}

$clickCount = 0
$dragValue = "none"
$scrollWheelTicks = 0

function Get-ScrollValue {
  return [Math]::Max($scrollBox.VerticalScroll.Value, $script:scrollWheelTicks)
}

$form = New-Object System.Windows.Forms.Form
$form.Text = $Title
$form.StartPosition = "Manual"
$form.Location = New-Object System.Drawing.Point(80, 80)
$form.ClientSize = New-Object System.Drawing.Size(420, 260)
$form.TopMost = $true

$textBox = New-Object System.Windows.Forms.TextBox
$textBox.Name = "InputText"
$textBox.Location = New-Object System.Drawing.Point(24, 24)
$textBox.Size = New-Object System.Drawing.Size(260, 28)

$button = New-Object System.Windows.Forms.Button
$button.Name = "ClickTarget"
$button.Text = "Click Target"
$button.Location = New-Object System.Drawing.Point(24, 68)
$button.Size = New-Object System.Drawing.Size(130, 36)

$scrollBox = New-Object System.Windows.Forms.Panel
$scrollBox.Name = "ScrollTarget"
$scrollBox.Location = New-Object System.Drawing.Point(24, 120)
$scrollBox.Size = New-Object System.Drawing.Size(180, 80)
$scrollBox.AutoScroll = $true
$scrollBox.BorderStyle = [System.Windows.Forms.BorderStyle]::FixedSingle
$scrollBox.TabStop = $true

$inner = New-Object System.Windows.Forms.Label
$inner.Text = ("Scroll Area`r`n" * 12)
$inner.AutoSize = $true
$scrollBox.Controls.Add($inner)

$dragBox = New-Object System.Windows.Forms.Label
$dragBox.Name = "DragTarget"
$dragBox.Text = "Drag Target"
$dragBox.TextAlign = [System.Drawing.ContentAlignment]::MiddleCenter
$dragBox.Location = New-Object System.Drawing.Point(230, 120)
$dragBox.Size = New-Object System.Drawing.Size(130, 80)
$dragBox.BorderStyle = [System.Windows.Forms.BorderStyle]::FixedSingle

$button.Add_Click({
  $script:clickCount += 1
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

$textBox.Add_TextChanged({
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

$scrollBox.Add_Scroll({
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

$scrollBox.Add_MouseDown({
  $scrollBox.Focus()
})

$scrollBox.Add_MouseWheel({
  if ($_.Delta -ne 0) {
    $script:scrollWheelTicks += [Math]::Max(1, [Math]::Abs([int]($_.Delta / 120)))
  }
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

$dragBox.Add_MouseUp({
  $script:dragValue = "dragged"
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

$form.Controls.Add($textBox)
$form.Controls.Add($button)
$form.Controls.Add($scrollBox)
$form.Controls.Add($dragBox)

$form.Add_Shown({
  $form.Activate()
  Write-State -ClickCount $script:clickCount -TextValue $textBox.Text -ScrollValue (Get-ScrollValue) -DragValue $script:dragValue -Ready $true
})

[System.Windows.Forms.Application]::EnableVisualStyles()
[System.Windows.Forms.Application]::Run($form)

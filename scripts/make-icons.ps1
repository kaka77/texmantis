# 生成 AKTools 应用图标（深蓝渐变圆角 + AK 字样）
# 用法: pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/make-icons.ps1
Add-Type -AssemblyName System.Drawing

function New-AkIcon([int]$size) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
  $rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
  $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $rect,
    [System.Drawing.Color]::FromArgb(255, 26, 30, 44),
    [System.Drawing.Color]::FromArgb(255, 64, 96, 172),
    90)
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $r = [int]($size * 0.22); $d = $r * 2
  $path.AddArc(0, 0, $d, $d, 180, 90)
  $path.AddArc($size - $d, 0, $d, $d, 270, 90)
  $path.AddArc($size - $d, $size - $d, $d, $d, 0, 90)
  $path.AddArc(0, $size - $d, $d, $d, 90, 90)
  $path.CloseFigure()
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.FillPath($brush, $path)
  $font = New-Object System.Drawing.Font('Segoe UI', [float]($size * 0.42), [System.Drawing.FontStyle]::Bold)
  $fmt = New-Object System.Drawing.StringFormat
  $fmt.Alignment = 'Center'; $fmt.LineAlignment = 'Center'
  $rectF = New-Object System.Drawing.RectangleF(0, 0, $size, $size)
  $g.DrawString('AK', $font, [System.Drawing.Brushes]::White, $rectF, $fmt)
  $g.Dispose()
  return $bmp
}

$out = Join-Path $PSScriptRoot '..\src-tauri\icons'
New-Item -ItemType Directory -Force $out | Out-Null

foreach ($s in 32, 128, 256, 512) {
  $bmp = New-AkIcon $s
  switch ($s) {
    512 { $bmp.Save("$out\icon.png", [System.Drawing.Imaging.ImageFormat]::Png) }
    256 { $bmp.Save("$out\128x128@2x.png", [System.Drawing.Imaging.ImageFormat]::Png) }
    128 { $bmp.Save("$out\128x128.png", [System.Drawing.Imaging.ImageFormat]::Png) }
    32  { $bmp.Save("$out\32x32.png", [System.Drawing.Imaging.ImageFormat]::Png) }
  }
  if ($s -eq 512) {
    $hicon = $bmp.GetHicon()
    $icon = [System.Drawing.Icon]::FromHandle($hicon)
    $fs = [System.IO.File]::Create("$out\icon.ico")
    $icon.Save($fs); $fs.Close()
  }
  $bmp.Dispose()
}
Write-Host "icons generated in $out"
Get-ChildItem $out | Select-Object Name, Length

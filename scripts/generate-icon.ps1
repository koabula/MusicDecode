$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskIconDir = Join-Path $taskRoot 'src-tauri\icons'
New-Item -ItemType Directory -Path $taskIconDir -Force | Out-Null
$taskBitmap = [System.Drawing.Bitmap]::new(256,256)
$taskGraphics = [System.Drawing.Graphics]::FromImage($taskBitmap)
$taskGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$taskGraphics.Clear([System.Drawing.Color]::Transparent)
$taskShape = [System.Drawing.Drawing2D.GraphicsPath]::new()
$taskShape.AddArc(4,4,96,96,180,90)
$taskShape.AddArc(156,4,96,96,270,90)
$taskShape.AddArc(156,156,96,96,0,90)
$taskShape.AddArc(4,156,96,96,90,90)
$taskShape.CloseFigure()
$taskBackground = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#e9edf0'))
$taskAccent = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#4e7f77'))
$taskPen = [System.Drawing.Pen]::new($taskAccent,13)
$taskPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$taskPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$taskGraphics.FillPath($taskBackground,$taskShape)
$taskGraphics.DrawLine($taskPen,105,171,105,79)
$taskGraphics.DrawLine($taskPen,105,79,183,62)
$taskGraphics.DrawLine($taskPen,183,62,183,153)
$taskGraphics.DrawLine($taskPen,105,106,183,89)
$taskGraphics.FillEllipse($taskAccent,64,153,47,36)
$taskGraphics.FillEllipse($taskAccent,142,135,47,36)
$taskPng = Join-Path $taskIconDir 'icon.png'
$taskBitmap.Save($taskPng,[System.Drawing.Imaging.ImageFormat]::Png)
$taskPngBytes = [System.IO.File]::ReadAllBytes($taskPng)
$taskStream = [System.IO.File]::Create((Join-Path $taskIconDir 'icon.ico'))
$taskWriter = [System.IO.BinaryWriter]::new($taskStream)
try {
    $taskWriter.Write([byte[]]@(0,0,1,0,1,0,0,0,0,0,1,0,32,0))
    $taskWriter.Write([uint32]$taskPngBytes.Length)
    $taskWriter.Write([uint32]22)
    $taskWriter.Write($taskPngBytes)
} finally { $taskWriter.Dispose(); $taskGraphics.Dispose(); $taskBitmap.Dispose(); $taskShape.Dispose(); $taskPen.Dispose(); $taskBackground.Dispose(); $taskAccent.Dispose() }

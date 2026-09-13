param(
    [Parameter(Mandatory = $true)]
    [string]$AudioRoot,
    [Parameter(Mandatory = $true)]
    [string]$SoundFont
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$assetsRoot = Join-Path $PSScriptRoot 'app\src\main\assets'

$expected = @{
    zenonia1 = 51
    zenonia2 = 16
    zenonia3 = 17
}

foreach ($name in $expected.Keys) {
    $source = Join-Path $AudioRoot $name
    if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        throw "Missing audio directory: $source"
    }
    $files = @(Get-ChildItem -LiteralPath $source -File)
    if ($files.Count -ne $expected[$name]) {
        throw "$name requires $($expected[$name]) files, found $($files.Count)"
    }
    $destination = Join-Path $assetsRoot $name
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath $files.FullName -Destination $destination -Force
}

if (-not (Test-Path -LiteralPath $SoundFont -PathType Leaf)) {
    throw "Missing soundfont: $SoundFont"
}
Copy-Item -LiteralPath $SoundFont -Destination (Join-Path $projectRoot 'wie_midi\soundfont.sf2') -Force

Write-Host 'Local Zenonia audio and MIDI soundfont imported.'

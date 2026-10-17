# Makes a new game from a template of this SDK:
#     new-game MyGame          a C# game in .\MyGame
#     new-game MyGame cpp      a C++ game (CMake)
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [ValidateSet("cs", "cpp")][string]$Lang = "cs",
    [string]$Dir = (Join-Path (Get-Location) $Name)
)
$ErrorActionPreference = "Stop"

if ($Name -notmatch '^[A-Za-z][A-Za-z0-9_]*$') {
    Write-Error "The name is a Latin letter then letters, digits or _ (it becomes the exe name): $Name"
}
if (Test-Path $Dir) { Write-Error "$Dir already exists" }

$sdk = $PSScriptRoot
$version = (Get-Content (Join-Path $sdk "VERSION") -Raw).Trim()
$template = Join-Path $sdk ("templates/" + $(if ($Lang -eq "cpp") { "cpp" } else { "csharp" }))
Copy-Item -Recurse $template $Dir

$utf8 = New-Object System.Text.UTF8Encoding($false)
@(Get-ChildItem -Recurse -File $Dir) | ForEach-Object {
    $text = [IO.File]::ReadAllText($_.FullName)
    $text = $text.Replace("__NAME__", $Name).Replace("__SDK__", $sdk.Replace("\", "/")).Replace("__VERSION__", $version)
    [IO.File]::WriteAllText($_.FullName, $text, $utf8)
    if ($_.Name.Contains("__NAME__")) { Rename-Item $_.FullName $_.Name.Replace("__NAME__", $Name) }
}

Write-Host "Done: $Dir"
if ($Lang -eq "cpp") {
    Write-Host "cd $Name"
    Write-Host "cmake -S . -B build"
    Write-Host "cmake --build build --config Release"
    Write-Host ".\build\Release\$Name.exe"
} else {
    Write-Host "cd $Name"
    Write-Host "dotnet run"
}

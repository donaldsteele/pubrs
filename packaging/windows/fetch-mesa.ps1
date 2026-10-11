# Downloads the pinned mesa-dist-win build, checks its SHA-256 and copies the
# x64 llvmpipe OpenGL driver (opengl32.dll, libgallium_wgl.dll) and the
# third-party notices to -Dest. The release workflow puts them in the MSI;
# CI runs the GUI self-test through them. See docs/adr/0011-gui-adapter.md.
param(
    [Parameter(Mandatory)] [string] $Dest,
    [string] $Cache = (Join-Path ([IO.Path]::GetTempPath()) 'pubrs-mesa')
)
$ErrorActionPreference = 'Stop'
$version = '26.2.4'
$sha256 = '351FC8C8B695878FFB3EAA044B3EAD08672A48B1A045E3C3E3975811DF0F6695'
$name = "mesa3d-$version-release-msvc.7z"
$archive = Join-Path $Cache $name
New-Item -ItemType Directory -Force $Cache, $Dest | Out-Null
if (-not (Test-Path $archive) -or (Get-FileHash $archive -Algorithm SHA256).Hash -ne $sha256) {
    curl.exe -fsSL -o $archive "https://github.com/pal1000/mesa-dist-win/releases/download/$version/$name"
    if ($LASTEXITCODE -ne 0) { throw "downloading $name failed" }
}
$hash = (Get-FileHash $archive -Algorithm SHA256).Hash
if ($hash -ne $sha256) { throw "$name has SHA-256 $hash, expected $sha256" }
$x = Join-Path $Cache 'x'
7z x $archive "-o$x" 'x64\opengl32.dll' 'x64\libgallium_wgl.dll' -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw "extracting $name failed" }
Copy-Item (Join-Path $x 'x64\opengl32.dll'), (Join-Path $x 'x64\libgallium_wgl.dll') $Dest
Copy-Item (Join-Path $PSScriptRoot 'mesa\THIRD-PARTY-NOTICES.txt') $Dest
Get-ChildItem $Dest | ForEach-Object { '{0,-28} {1,12:N0}' -f $_.Name, $_.Length }

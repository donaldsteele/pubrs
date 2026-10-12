# Fails if an -Exe file needs the Visual C++ runtime DLLs (VCRUNTIME140.dll,
# MSVCP140.dll, ucrtbase.dll or the api-ms-win-crt-* forwarders). pubrs links
# the C runtime statically (.cargo/config.toml, ADR 0010) so it starts on a
# clean Windows install; CI runners have the runtime installed, so a missing
# DLL never shows up when the program runs there. -Report files (the bundled
# Mesa) are listed without failing.
param([string[]] $Exe = @(), [string[]] $Report = @())
$ErrorActionPreference = 'Stop'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$dumpbin = & $vswhere -latest -products * -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
if (!$dumpbin) { throw "dumpbin.exe not found" }
function Get-Dlls([string] $file) {
    $out = & $dumpbin /nologo /dependents $file
    if ($LASTEXITCODE -ne 0) { throw "dumpbin failed on $file" }
    @($out | ForEach-Object { if ($_ -match '^\s+(\S+\.dll)\s*$') { $Matches[1] } })
}
$failed = $false
foreach ($check in @(@{ Files = $Exe; Fail = $true }, @{ Files = $Report; Fail = $false })) {
    foreach ($file in $check.Files) {
        $dlls = Get-Dlls $file
        # Every program imports at least KERNEL32.dll; none means the output was not understood.
        if ($dlls.Count -eq 0) { throw "no DLL imports found in $file" }
        $runtime = @($dlls | Where-Object { $_ -match '^(vcruntime|msvcp|ucrtbase|api-ms-win-crt-)' })
        "{0}: {1}" -f $file, ($dlls -join ', ')
        if ($runtime.Count -gt 0) {
            "  needs the C runtime: {0}" -f ($runtime -join ', ')
            if ($check.Fail) { $failed = $true }
        }
    }
}
if ($failed) { throw "an executable needs the Visual C++ runtime DLLs" }

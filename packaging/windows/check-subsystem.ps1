# Fails unless each -Gui exe is a Windows GUI program (no console window) and
# each -Console exe a console program. pubrs-app must not open a command
# prompt; pubrs and pubrs-mcp must keep stdio (MCP clients talk over it).
param([string[]] $Gui = @(), [string[]] $Console = @())
$ErrorActionPreference = 'Stop'
function Get-Subsystem([string] $exe) {
    $head = New-Object byte[] 4096
    $f = [IO.File]::OpenRead($exe)
    try { [void]$f.Read($head, 0, $head.Length) } finally { $f.Dispose() }
    $pe = [BitConverter]::ToInt32($head, 0x3C)
    # PE signature (4 bytes) + COFF header (20) + Subsystem offset in the optional header (68).
    [BitConverter]::ToUInt16($head, $pe + 24 + 68)
}
$names = @{ 2 = 'GUI'; 3 = 'console' }
$failed = $false
foreach ($check in @(@{ Files = $Gui; Want = 2 }, @{ Files = $Console; Want = 3 })) {
    foreach ($exe in $check.Files) {
        $got = Get-Subsystem $exe
        "{0}: {1}" -f $exe, ($names[[int]$got] ?? "subsystem $got")
        if ($got -ne $check.Want) { $failed = $true }
    }
}
if ($failed) { throw "an executable has the wrong subsystem" }

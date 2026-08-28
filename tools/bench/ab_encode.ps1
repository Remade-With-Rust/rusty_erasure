# Scalar-vs-accel A/B on the encode path — codec-measurement compliant:
# pinned (core 2, High), CPU time, ABBA arm order alternating per round,
# work parity asserted via the deterministic parity checksum (byte-identical
# kernels => identical checksum), reps per (cell, arm) sized for >= ~5-15 s.
#
# Usage: tools\bench\ab_encode.ps1   (after: cargo build --release -p rusty_erasure-cli)

param([int]$Rounds = 3, [string]$Exe = "target\release\rerasure.exe")

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { throw "build first: cargo build --release -p rusty_erasure-cli" }

# k, p, len, scalar_reps, auto_reps
$cells = @(
    @(4, 2, 4096, 400000, 3000000),
    @(10, 4, 65536, 6000, 100000),
    @(10, 4, 1048576, 400, 6000)
)

function Run-Arm([string]$exe, [int]$k, [int]$p, [int]$len, [int]$reps, [string]$kern) {
    $out = "$env:TEMP\ab_out.txt"
    $args = "bench --k $k --p $p --len $len --reps $reps --kernels $kern"
    $proc = Start-Process -FilePath $exe -ArgumentList $args -PassThru -WindowStyle Hidden -RedirectStandardOutput $out
    $null = $proc.Handle
    $proc.ProcessorAffinity = [IntPtr]4
    $proc.PriorityClass = 'High'
    $proc.WaitForExit()
    $cpu = $proc.TotalProcessorTime.TotalSeconds
    $chk = ((Get-Content $out | Select-String "checksum=(0x[0-9a-f]+)").Matches[0].Groups[1].Value)
    $gbps = [double]$k * $len * $reps / $cpu / 1e9
    return @{ cpu = $cpu; gbps = $gbps; chk = $chk }
}

foreach ($cell in $cells) {
    $k = $cell[0]; $p = $cell[1]; $len = $cell[2]; $sreps = $cell[3]; $areps = $cell[4]
    $s = @(); $a = @(); $chks = @{}
    for ($r = 0; $r -lt $Rounds; $r++) {
        # ABBA: alternate which arm leads each round.
        $order = if ($r % 2 -eq 0) { @("auto", "scalar") } else { @("scalar", "auto") }
        foreach ($arm in $order) {
            $reps = if ($arm -eq "scalar") { $sreps } else { $areps }
            $res = Run-Arm $Exe $k $p $len $reps $arm
            $chks[$res.chk] = $true
            if ($arm -eq "scalar") { $s += $res.gbps } else { $a += $res.gbps }
            "round {0} {1,-6} cpu_s={2:N2} src_GBps={3:N3}" -f $r, $arm, $res.cpu, $res.gbps
        }
    }
    if ($chks.Count -ne 1) { throw "WORK PARITY VIOLATED: arms produced different checksums" }
    $sb = ($s | Measure-Object -Maximum).Maximum; $ab = ($a | Measure-Object -Maximum).Maximum
    $sm = ($s | Sort-Object)[[int](($s.Count - 1) / 2)]; $am = ($a | Sort-Object)[[int](($a.Count - 1) / 2)]
    ""
    "CELL k=$k p=$p len=$len  scalar best={0:N3} med={1:N3}  accel best={2:N3} med={3:N3}  speedup(best)={4:N1}x" -f $sb, $sm, $ab, $am, ($ab / $sb)
    "method: pinned core2 High, CPU-time, ABBA lead alternated, $Rounds rounds/arm, work parity via identical checksum, source-bytes basis"
    ""
}

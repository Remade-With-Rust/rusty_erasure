# Scalar encode baseline harness — codec-measurement §1–§2 compliant:
# pinned to one core (not core 0), High priority, CPU time (not wall), N runs
# of the identical binary (a single-arm baseline: the run-to-run spread IS the
# null floor). Prints a method line with every figure.
#
# Usage: tools\bench\scalar_baseline.ps1   (after: cargo build --release --example scalar_baseline)

param(
    [int]$N = 7,
    [string]$Exe = "target\release\examples\scalar_baseline.exe"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { throw "build first: cargo build --release -p rusty_erasure-core --example scalar_baseline" }

# (k, p, shard_len, reps) — reps sized so each run's work is >= ~15 s scalar.
$cells = @(
    @(4, 2, 4096, 400000),
    @(10, 4, 65536, 6000),
    @(10, 4, 1048576, 400)
)

foreach ($cell in $cells) {
    $k = $cell[0]; $p = $cell[1]; $len = $cell[2]; $reps = $cell[3]
    $srcBytes = [double]$k * $len * $reps
    $rates = @()
    $checks = @{}
    for ($i = 0; $i -lt $N; $i++) {
        $proc = Start-Process -FilePath $Exe -ArgumentList "$k $p $len $reps" -PassThru -WindowStyle Hidden -RedirectStandardOutput "$env:TEMP\sb_out.txt"
        $null = $proc.Handle   # must precede WaitForExit or TotalProcessorTime reads empty
        $proc.ProcessorAffinity = [IntPtr]4   # one core, not core 0
        $proc.PriorityClass = 'High'
        $proc.WaitForExit()
        $cpu = $proc.TotalProcessorTime.TotalSeconds
        $gbps = $srcBytes / $cpu / 1e9
        $rates += $gbps
        $line = (Get-Content "$env:TEMP\sb_out.txt" | Select-String "checksum").Line
        $checks[$line] = $true
        "{0} k={1} p={2} len={3}: cpu_s={4:N2} src_GBps={5:N3}" -f ($i + 1), $k, $p, $len, $cpu, $gbps
    }
    if ($checks.Count -ne 1) { throw "determinism violated: differing checksums across runs" }
    $sorted = $rates | Sort-Object
    $best = $sorted[-1]; $median = $sorted[[int](($N - 1) / 2)]; $worst = $sorted[0]
    $floor = ($best - $worst) / $median * 100
    ""
    "CELL k=$k p=$p len=$len  best={0:N3} GB/s  median={1:N3}  spread_floor={2:N1}%" -f $best, $median, $floor
    "method: pinned core2 High, CPU-time (TotalProcessorTime), N=$N identical runs (single-arm baseline; spread = null floor), work asserted identical via checksum, source-bytes basis"
    ""
}

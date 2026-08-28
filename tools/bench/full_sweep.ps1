# The full ERASCORP performance sweep (mission plan section 7.2): every
# scenario cell, ours-vs-ISA-L interleaved ABBA where a fair cross arm exists,
# pinned core 2 High, CPU-time for single-thread cells. Run on a quiet box
# after the correctness battery. ASCII-only file (PowerShell 5.1 reads
# BOM-less scripts as ANSI).
#
# Prereqs: cargo build --release -p rusty_erasure-cli; ~/perf_arm + ~/isa-l in WSL.

param([int]$Rounds = 3, [string]$Exe = "target\release\rerasure.exe")
$ErrorActionPreference = "Stop"

function Run-Ours([string]$argline) {
    $out = "$env:TEMP\fs_out.txt"
    $proc = Start-Process -FilePath $Exe -ArgumentList $argline -PassThru -WindowStyle Hidden -RedirectStandardOutput $out
    $null = $proc.Handle
    $proc.ProcessorAffinity = [IntPtr]4
    $proc.PriorityClass = 'High'
    $proc.WaitForExit()
    $cpu = $proc.TotalProcessorTime.TotalSeconds
    $work = (Get-Content $out | Select-String "source_bytes=(\d+)").Matches[0].Groups[1].Value
    $chk = (Get-Content $out | Select-String "checksum=(0x[0-9a-f]+)").Matches[0].Groups[1].Value
    return @{ cpu = $cpu; gbps = [double]$work / $cpu / 1e9; chk = $chk }
}

function Run-IsalEncode([int]$k, [int]$p, [int]$len, [long]$reps) {
    $line = wsl -e sh -c "taskset -c 2 ~/perf_arm $k $p $len $reps 0"
    if ($line -match 'src_GBps=([0-9.]+)') { return [double]$Matches[1] } else { return 0 }
}

Write-Host "=== S1/S2/S3/S5 encode cells (ours GFNI vs ISA-L dispatched, ABBA x$Rounds) ==="
$cells = @(
    @("S1", 4, 2, 4096, 3000000, 10000000),
    @("S2", 10, 4, 65536, 100000, 100000),
    @("S3", 10, 4, 1048576, 6000, 5000),
    @("S5a", 17, 3, 65536, 60000, 60000),
    @("S5b", 20, 8, 65536, 30000, 30000)
)
foreach ($c in $cells) {
    $name = $c[0]; $k = $c[1]; $p = $c[2]; $len = $c[3]; $oreps = $c[4]; $ireps = $c[5]
    $ours = @(); $isal = @()
    for ($r = 0; $r -lt $Rounds; $r++) {
        $order = if ($r % 2 -eq 0) { @("o", "i") } else { @("i", "o") }
        foreach ($a in $order) {
            if ($a -eq "o") { $ours += (Run-Ours "bench --k $k --p $p --len $len --reps $oreps --kernels auto").gbps }
            else { $isal += Run-IsalEncode $k $p $len $ireps }
        }
    }
    $ob = ($ours | Measure-Object -Maximum).Maximum; $ib = ($isal | Measure-Object -Maximum).Maximum
    "{0} k={1} p={2} len={3}: OURS best={4:N2} GB/s  ISA-L best={5:N2}  ratio={6:N3}" -f $name, $k, $p, $len, $ob, $ib, ($ob / $ib)
}

Write-Host ""
Write-Host "=== S4 cache sweep (10,4), per-len GB/s, both arms, best-of-2 ==="
foreach ($len in @(4096, 65536, 262144, 1048576, 4194304, 16777216)) {
    $reps = [Math]::Max(3, [long](20e9 / (10.0 * $len)))
    $o = @(); $i = @()
    foreach ($r in 0..1) {
        $o += (Run-Ours "bench --k 10 --p 4 --len $len --reps $reps --kernels auto").gbps
        $i += Run-IsalEncode 10 4 $len $reps
    }
    "len={0,9}: OURS {1:N2}  ISA-L {2:N2}" -f $len, ($o | Measure-Object -Maximum).Maximum, ($i | Measure-Object -Maximum).Maximum
}

Write-Host ""
Write-Host "=== S6 recovery (10,4,64K), losses 1/2/4, ours as-shipped (per-call matrix) ==="
foreach ($l in @(1, 2, 4)) {
    $res = @(); foreach ($r in 0..1) { $res += (Run-Ours "bench --op recover --k 10 --p 4 --len 65536 --losses $l --reps 60000").gbps }
    "losses=$l : OURS {0:N2} GB/s source-basis" -f ($res | Measure-Object -Maximum).Maximum
    wsl -e sh -c "cd ~/isa-l; taskset -c 2 ./erasure_code/erasure_code_perf -k 10 -p 4 -e $l -s 64K 2>/dev/null | grep decode"
}

Write-Host ""
Write-Host "=== S7 update (10,4,64K) full sequence per rep ==="
$res = @(); foreach ($r in 0..1) { $res += (Run-Ours "bench --op update --k 10 --p 4 --len 65536 --reps 40000").gbps }
"update: OURS {0:N2} GB/s source-basis" -f ($res | Measure-Object -Maximum).Maximum

Write-Host ""
Write-Host "=== S8 tail length (10,4,65567) ours only (their SIMD contract excludes tails) ==="
$res = @(); foreach ($r in 0..1) { $res += (Run-Ours "bench --k 10 --p 4 --len 65567 --reps 100000 --kernels auto").gbps }
"tail: OURS {0:N2} GB/s" -f ($res | Measure-Object -Maximum).Maximum

Write-Host ""
Write-Host "=== S9 RAID (8 sources, 64K) ==="
foreach ($op in @("xor", "pq")) {
    $res = @(); foreach ($r in 0..1) { $res += (Run-Ours "bench --op $op --k 8 --p 1 --len 65536 --reps 200000").gbps }
    "$op : OURS {0:N2} GB/s source-basis" -f ($res | Measure-Object -Maximum).Maximum
}
wsl -e sh -c "cd ~/isa-l; taskset -c 2 ./raid/xor_gen_perf 2>/dev/null | tail -1; taskset -c 2 ./raid/pq_gen_perf 2>/dev/null | tail -1"

Write-Host ""
Write-Host "=== S10 multi-stripe (10,4,64K x 47 stripes), wall basis ==="
foreach ($t in @(1, 8, 24)) {
    $out = "$env:TEMP\fs_out.txt"
    & $Exe bench --k 10 --p 4 --len 65536 --reps 1200 --stripes 47 --threads $t > $out
    (Get-Content $out | Select-String "aggregate").Line -replace "aggregate: ", "threads=$t "
}
Write-Host ""
Write-Host "SWEEP COMPLETE"

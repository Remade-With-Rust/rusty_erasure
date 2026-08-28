# The §6.4 cross-cutting build gate, runnable locally on Windows.
# Libraries check on all 8 shipped targets; the CLI checks on the host only
# (a CLI has no wasm deliverable). Run before every push.

$ErrorActionPreference = "Stop"

$targets = @(
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "wasm32-unknown-unknown"
)

$libs = @("rusty_erasure-core", "rusty_erasure-accel", "rusty_erasure", "rusty_erasure-alloc")

$failed = @()
foreach ($t in $targets) {
    Write-Host "== cargo check (libs) --target $t" -ForegroundColor Cyan
    $pkgArgs = @(); foreach ($l in $libs) { $pkgArgs += @("-p", $l) }
    cargo check @pkgArgs --target $t
    if ($LASTEXITCODE -ne 0) { $failed += $t }
    # The pure-safe build must also hold on every target.
    cargo check -p rusty_erasure --no-default-features --target $t
    if ($LASTEXITCODE -ne 0) { $failed += "$t (--no-default-features)" }
}

Write-Host "== cargo check (workspace incl. CLI) on host" -ForegroundColor Cyan
cargo check --workspace
if ($LASTEXITCODE -ne 0) { $failed += "host workspace" }

if ($failed.Count -gt 0) {
    Write-Host "CHECK MATRIX FAILED:" -ForegroundColor Red
    $failed | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    exit 1
}
Write-Host "Check matrix green on all $($targets.Count) targets." -ForegroundColor Green

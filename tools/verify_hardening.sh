#!/usr/bin/env bash
# Binary hardening verification (use-protection-please H-31).
#
# Checks the SHIPPED binary actually carries the mitigations, rather than
# trusting that the toolchain defaults held. Run in CI on every push so a
# linker-flag change cannot silently un-harden the release artifact.
#
# Usage: tools/verify_hardening.sh <path-to-elf-binary>
set -euo pipefail

BIN="${1:?usage: verify_hardening.sh <binary>}"
[ -f "$BIN" ] || { echo "no such binary: $BIN" >&2; exit 2; }

fail=0
check() { # name, condition-already-evaluated ("yes"/"no"), detail
    if [ "$2" = yes ]; then
        printf '  PASS  %-28s %s\n' "$1" "$3"
    else
        printf '  FAIL  %-28s %s\n' "$1" "$3"
        fail=1
    fi
}

echo "hardening report: $BIN"

# PIE: ELF type DYN (an ET_EXEC binary loads at a fixed address, defeating ASLR).
if readelf -h "$BIN" | grep -q 'Type:.*DYN'; then pie=yes; else pie=no; fi
check "PIE (ASLR-capable)" "$pie" "ELF type DYN"

# Full RELRO: the GNU_RELRO segment plus BIND_NOW, so the GOT is mapped
# read-only after startup and cannot be overwritten to hijack a call.
if readelf -lW "$BIN" | grep -q 'GNU_RELRO'; then relro=yes; else relro=no; fi
if readelf -dW "$BIN" | grep -qE 'BIND_NOW|FLAGS.*NOW'; then now=yes; else now=no; fi
check "RELRO segment" "$relro" "GNU_RELRO present"
check "BIND_NOW (full RELRO)" "$now" "eager binding"

# NX: the stack segment must not be executable.
stack=$(readelf -lW "$BIN" | grep 'GNU_STACK' || true)
if [ -z "$stack" ] || ! echo "$stack" | grep -qE 'RWE'; then nx=yes; else nx=no; fi
check "NX (no exec stack)" "$nx" "GNU_STACK not RWE"

# Auditable: the dependency list embedded by `cargo auditable`, so the shipped
# artifact can be scanned for advisories without its source tree.
if readelf -SW "$BIN" | grep -q '.dep-v0'; then aud=yes; else aud=no; fi
check "auditable dep list" "$aud" ".dep-v0 section"

if [ "$fail" -ne 0 ]; then
    echo "HARDENING CHECK FAILED" >&2
    exit 1
fi
echo "all hardening checks passed"

#!/usr/bin/env bash
# Derived from aeyakovenko/percolator at d8dad98 (Apache-2.0); modified by QuantuLabs.
# Sequential, bounded Kani proofs. Never overwrite the historical matrix.
set -euo pipefail

# Check before executing any external tool (in particular on macOS).
if [[ ${OSTYPE:-} != linux* ]]; then
    echo 'Linux only: use a memory-limited Linux container or CI; see README.md.' >&2
    exit 1
fi
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ $(timeout --version) == *'GNU coreutils'* ]] || {
    echo 'GNU timeout is required.' >&2
    exit 1
}

package=${KANI_PACKAGE:-colada-engine}
case "$package" in
    colada-engine) ;;
    colada) ;;
    *) echo "Unsupported KANI_PACKAGE: $package" >&2; exit 1 ;;
esac
object_bits=${KANI_OBJECT_BITS:-12}
case "$object_bits" in
    12|13|14) ;;
    *) echo 'KANI_OBJECT_BITS must be 12, 13 or 14.' >&2; exit 1 ;;
esac

timeout_seconds=${KANI_TIMEOUT_SECONDS:-600}
[[ $timeout_seconds =~ ^[0-9]{2,4}$ ]] && (( 10#$timeout_seconds >= 60 && 10#$timeout_seconds <= 3600 )) || {
    echo 'KANI_TIMEOUT_SECONDS must be an integer from 60 to 3600.' >&2
    exit 1
}
timeout_seconds=$((10#$timeout_seconds))
proofs=$(python3 scripts/kani_ci.py list "$package")
[[ -n $proofs ]] || { echo 'No proof harnesses found.' >&2; exit 1; }
mapfile -t available <<< "$proofs"
if (( $# == 0 )); then
    set -- "${available[@]}"
fi
# Validate every argument before starting any proof; filters cannot produce PASS.
for proof in "$@"; do
    found=false
    for name in "${available[@]}"; do
        if [[ $proof == "$name" ]]; then found=true; break; fi
    done
    $found || { echo "Unknown harness: $proof" >&2; exit 1; }
done

output_root="${CARGO_TARGET_DIR:-target}/kani-audit"
mkdir -p -- "$output_root"
output=$(mktemp -d "$output_root/run-XXXXXXXX")
printf 'proof\ttime_s\tstatus\n' > "$output/results.tsv"
if [[ -n ${KANI_RESULTS_FILE:-} ]]; then
    mkdir -p -- "$(dirname -- "$KANI_RESULTS_FILE")"
    printf 'proof\ttime_s\tstatus\n' > "$KANI_RESULTS_FILE"
fi
printf 'Logs and incremental results: %s\n' "$output"
failed=0
for proof in "$@"; do
    printf 'Running %s\n' "$proof"
    start=$(date +%s)
    # Limits are set and verified in the SAME subshell that executes cargo.
    # Keep an inherited smaller hard limit rather than trying to raise it.
    if (
        set -e
        cap=8000000
        hard=$(ulimit -Hv)
        if [[ $hard != unlimited ]] && (( hard < cap )); then cap=$hard; fi
        (( cap > 0 )) || exit 1
        ulimit -Sv "$cap" || exit 1
        ulimit -Hv "$cap" || exit 1
        soft=$(ulimit -Sv)
        hard=$(ulimit -Hv)
        [[ $soft =~ ^[0-9]+$ && $hard =~ ^[0-9]+$ ]] || exit 1
        (( soft > 0 && soft <= 8000000 && hard > 0 && hard <= 8000000 )) || exit 1
        exec timeout --kill-after=5s "${timeout_seconds}s" cargo kani -p "$package" \
            --harness "$proof" --exact --jobs 1 --default-unwind 70 \
            -Z unstable-options --cbmc-args --object-bits "$object_bits"
    ) > "$output/$proof.log" 2>&1; then
        code=0
    else
        code=$?
    fi
    status=$(python3 scripts/kani_ci.py classify "$code" "$output/$proof.log")
    elapsed=$(( $(date +%s) - start ))
    printf '%s\t%s\t%s\n' "$proof" "$elapsed" "$status" >> "$output/results.tsv"
    if [[ -n ${KANI_RESULTS_FILE:-} ]]; then
        printf '%s\t%s\t%s\n' "$proof" "$elapsed" "$status" >> "$KANI_RESULTS_FILE"
    fi
    printf '%s: %s (%ss)\n' "$proof" "$status" "$elapsed"
    if [[ $status != PASS ]]; then
        failed=1
        [[ ${KANI_CONTINUE_ON_FAILURE:-0} == 1 ]] || exit 1
    fi
done
exit "$failed"

#!/usr/bin/env bash
# Acceptance of the signer's in-token backup storage (CTAP 2.1 large
# blobs) against libfido2: what one writes under the signer's resident
# credential, the other reads back unchanged. Cleans up its credential.
# Run in a terminal with one token plugged in; enter the PIN when asked
# and touch the token whenever it blinks.
#   scripts/fido-blob-acceptance.sh [path/to/fido-probe]
set -euo pipefail
probe=${1:-target/debug/fido-probe}
dev=$(fido2-token -L | head -1 | cut -d: -f1)
[ -n "$dev" ] || { echo "no FIDO2 token" >&2; exit 1; }
read -rsp "token PIN: " FIDO_PIN; echo
export FIDO_PIN
rp=air-gap-signer
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
b64() { xxd -r -p | base64 -w0; }

fail=0
# This RP's resident credentials on the token (empty when none).
ours() { fido2-token -L -k "$rp" "$dev" 2>&1 | grep -v NO_CREDENTIALS || true; }

check() { if [ "$2" = "$3" ] && [ -n "$2" ]; then echo "PASS $1"; else echo "FAIL $1: $2 vs $3"; fail=1; fi; }

echo "the token must start with no credential for $rp"
before=$(ours)
[ -z "$before" ] || { echo "FAIL: leftover credentials, delete them first:"; echo "$before"; exit 1; }

echo "enrol a resident credential (signer)"
id=$("$probe" enrol)
idb=$(echo "$id" | b64)

echo "1/2 written by the signer, read by libfido2"
head -c 300 /dev/urandom > "$work/a"
"$probe" blob-put "$id" "$work/a" >/dev/null
fido2-token -G -b -n "$rp" -i "$idb" "$work/a.back" "$dev"
check "libfido2 reads what the signer wrote" "$(xxd -p -c999 "$work/a")" "$(xxd -p -c999 "$work/a.back")"

echo "2/2 written by libfido2, read by the signer"
head -c 300 /dev/urandom > "$work/b"
fido2-token -S -b -n "$rp" -i "$idb" "$work/b" "$dev"
read -r got_id got_data < <("$probe" blob-get)
check "the signer finds its credential without being told" "$got_id" "$id"
check "the signer reads what libfido2 wrote" "$got_data" "$(xxd -p -c999 "$work/b")"

echo "cleanup: delete the blob and the credential (libfido2)"
fido2-token -D -b -n "$rp" -i "$idb" "$dev"
fido2-token -D -i "$idb" "$dev"
after=$(ours)
check "no credential left behind" "none" "${after:-none}"
# Control: with the credential gone the signer must find none.
refused=$("$probe" blob-get 2>&1 || true)
check "control: the signer finds no credential" "token said 0x2e" "$refused"
exit $fail

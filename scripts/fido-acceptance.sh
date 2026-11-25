#!/usr/bin/env bash
# Acceptance of the signer's FIDO2 client against libfido2: for the same
# credential and salt, the token must return the same hmac-secret to both.
# Run with one token plugged in; touch it each time it blinks (7 times).
#   scripts/fido-acceptance.sh [path/to/fido-probe]
set -euo pipefail
probe=${1:-target/debug/fido-probe}
dev=$(fido2-token -L | head -1 | cut -d: -f1)
[ -n "$dev" ] || { echo "no FIDO2 token" >&2; exit 1; }
rp=air-gap-signer
salt=$(head -c32 /dev/urandom | xxd -p -c64)
b64() { xxd -r -p | base64 -w0; }
hex() { base64 -d | xxd -p -c256; }
cdh=$(head -c32 /dev/zero | base64 -w0)

libfido2_secret() { # credential hex -> secret hex, via fido2-assert
  printf '%s\n%s\n%s\n%s\n' "$cdh" "$rp" "$(echo "$1" | b64)" "$(echo "$salt" | b64)" \
    | fido2-assert -G -h "$dev" | tail -1 | hex
}

fail=0
check() { # name, ours, theirs
  if [ "$2" = "$3" ] && [ -n "$2" ]; then echo "PASS $1"; else echo "FAIL $1: ours=$2 libfido2=$3"; fail=1; fi
}

echo "1/2 credential made by the signer's client"
echo "  touch: enrol"; id=$("$probe" enrol)
echo "  touch: secret (signer)"; ours=$("$probe" secret "$id" "$salt")
echo "  touch: secret (libfido2)"; theirs=$(libfido2_secret "$id")
check "signer credential, same secret" "$ours" "$theirs"

echo "2/2 credential made by libfido2"
echo "  touch: enrol (fido2-cred)"
uid=$(head -c16 /dev/urandom | base64 -w0)
id=$(printf '%s\n%s\n%s\n%s\n' "$cdh" "$rp" backup "$uid" | fido2-cred -M -h "$dev" | sed -n 5p | hex)
echo "  touch: secret (signer)"; ours=$("$probe" secret "$id" "$salt")
echo "  touch: secret (libfido2)"; theirs=$(libfido2_secret "$id")
check "libfido2 credential, same secret" "$ours" "$theirs"

# Control: a different salt must give a different secret.
echo "  touch: control (other salt)"
other=$("$probe" secret "$id" "$(head -c32 /dev/urandom | xxd -p -c64)")
if [ "$other" != "$ours" ]; then echo "PASS control: other salt, other secret"; else echo "FAIL control"; fail=1; fi
exit $fail

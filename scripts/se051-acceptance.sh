#!/usr/bin/env bash
# Acceptance of the SE051 key store against OpenSSL: a key created in the
# chip signs, and OpenSSL verifies the signature with the public key the
# device reports; a changed message must fail (control). Uses a scratch
# slot and erases it after.
#   scripts/se051-acceptance.sh <pi ssh host> [slot]
set -euo pipefail
host=$1; slot=${2:-3}
# Quote each argument: ssh would drop an empty one (the empty message).
pi() { ssh -o BatchMode=yes "$host" "sudo /tmp/se051-keys $(printf '%q ' "$@")"; }
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

[ "$(pi public "$slot")" = empty ] || { echo "slot $slot is in use; not touching it" >&2; exit 1; }
public=$(pi create "$slot")
# Only now is there a key of ours to erase, and only if it is still ours.
trap 'rm -rf "$work"; [ "$(pi public "$slot" 2>/dev/null)" = "$public" ] && pi delete "$slot" >/dev/null' EXIT
check_public=$(pi public "$slot")
[ "$public" = "$check_public" ] || { echo "FAIL public key read back differs"; exit 1; }

# Ed25519 public key as SubjectPublicKeyInfo (RFC 8410 prefix).
echo "302a300506032b6570032100$public" | xxd -r -p | openssl pkey -pubin -inform DER -out "$work/pub.pem"

fail=0
# The empty message is covered by the RFC 8032 vector in the unit tests
# (the OpenSSL CLI cannot read an empty -rawin file); 250 is the chip's
# limit.
for len in 1 32 250; do
  head -c "$len" /dev/urandom > "$work/msg"
  sig=$(pi sign "$slot" "$(xxd -p -c99999 "$work/msg")")
  echo "$sig" | xxd -r -p > "$work/sig"
  if openssl pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin -in "$work/msg" -sigfile "$work/sig" >/dev/null; then
    echo "PASS OpenSSL verifies a $len-byte message"
  else
    echo "FAIL OpenSSL rejects a $len-byte message"; fail=1
  fi
done
# Control: the last signature on a changed message must not verify.
printf 'x' >> "$work/msg"
if openssl pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin -in "$work/msg" -sigfile "$work/sig" >/dev/null 2>&1; then
  echo "FAIL control: a changed message verifies"; fail=1
else
  echo "PASS control: a changed message is rejected"
fi
# Control: past the limit the device refuses before reaching the chip,
# and the chip still answers afterwards.
too_long=$(head -c 251 /dev/urandom | xxd -p -c99999)
case "$(pi sign "$slot" "$too_long" 2>&1 || true)" in
  *"signs at most 250"*) echo "PASS control: 251 bytes refused before the chip";;
  *) echo "FAIL control: 251 bytes not refused cleanly"; fail=1;;
esac
[ "$(pi public "$slot")" = "$public" ] && echo "PASS the chip still answers" || { echo "FAIL the chip stopped answering"; fail=1; }
exit $fail

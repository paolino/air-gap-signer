# Bluetooth signer for Cardano preprod

This prototype runs on the Raspberry Pi 3 Model B with the Adafruit 1.3-inch TFT Bonnet, an SE051 wired on I²C, and microSD storage. It supports one ADA-only input and one output returning to the enrolled practice payment key, less the fee. The signer checks the original transaction body, the transaction ID and expiry, and requires local review and the joystick PIN. The desktop supplies the resolved input and chain-tip data. Use test ADA.

## Install

Download the Pi image (`.img.zst`), the CLI for your laptop, and `SHA256SUMS` from the same release. Match the hashes before flashing. Extract the Linux or Apple Silicon macOS archive to get `signer`. Windows x64 uses the supplied executable. The commands below use `signer` on the path. You can also run `./signer` or `.\signer.exe` from its directory.

Write the decompressed image to a spare microSD with your normal Raspberry Pi imaging tool. Flashing replaces the whole card. Preserve the original card, wallet records and recovery material when upgrading an enrolled board. A fresh release image contains no enrollment records or recovery archives. Put the card into the Pi, disconnect USB storage and the recovery token for boot, and apply power. The bonnet displays progress until **Bluetooth ready**.

On a fresh setup, press **#6 Keys**, tap Up once from slot 0, then **#5 Practice key**. Use **Create + back up** with your supported FIDO2 recovery token, then **Enroll signing PIN** and follow the prompts on the panel. Remove the recovery token when finished. Pairing the laptop requires the enrolled payment public key. For existing keys and backups, read the [hardware bench guide](dev/prototype-rig.md) before changing cards.

## Pair and set the clock

On the panel press **#5 Pair laptop**. On the laptop run:

```console
signer pair
signer sync-clock
```

Compare the six digits on both devices before accepting. Linux asks you to type `yes`, while Windows and macOS use the system pairing prompt. The client saves the panel identity and payment public key and rechecks them on every transfer. If the OS forgets the bond, reopen pairing and run `signer pair` again.

The Pi can start with an old time after losing power. Check the proposed UTC date and time against a clock you trust, then press **#5** to advance the panel clock. **#6** cancels. The transaction expiry checks still apply. An older panel image can pair successfully while lacking the clock command.

## Send and retrieve

Start with an unsigned Conway text envelope in `payment.txbody` and its matching `plan.json`. The [preprod bench guide](dev/prototype-rig.md#preprod-self-payment-on-the-bench) defines the plan fields. Keep both files exactly as sent until you retrieve the result.

```console
signer send --tx-body payment.txbody --plan plan.json
signer retrieve --tx-body payment.txbody --plan plan.json --out payment.signed
```

Send returns after the panel acknowledges the request. On the panel, **#6 Exit** declines at the first review prompt. To approve, review the fields, enter Sign mode, confirm the fee and enter the joystick PIN on the panel. Retrieve can run while review is pending. After approval it verifies the signature against the original body and enrolled key, then writes a signed Conway text envelope. A decline produces a message and leaves the output path free. Network submission is a separate owner action.

After a laptop disconnect, repeat **retrieve** with the same two input files. The panel holds the most recent result until another request replaces it or the service restarts. Retrieve only reads. Resending the same request while it is still on the panel reuses that review. Use a new output path to preserve an existing file. The combined `signer sign --tx-body payment.txbody --out payment.witness` command remains available for workflows that require a separate Cardano witness.

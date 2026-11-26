//! Experiment B for the secure project definition ("Where the phone
//! factor is enforced"): can the SE051 confine signing to commands the
//! phone authenticated — this key, this digest, once?
//!
//! The phone holds an AES key the chip knows as an authentication
//! object. It opens an SCP03 session with command MACs (C-MAC) and MACs
//! the sign command for one key and one digest; the controller only
//! relays bytes. The key's policy grants signing to that credential with
//! secure messaging required. The chip's answers are recorded for: the
//! session without MACs, the MAC'd command, its replay, a changed digest
//! under the phone's MAC, and a static UserID credential.
//!
//! Everything lives in a scratch range. A cleaner UserID credential
//! whose policy lets it delete itself is created and proven deletable
//! first; it then deletes the key, the phone credential and itself.
//! Refuses to start if any scratch object exists; exits non-zero on any
//! unexpected answer.
//!
//! Result on the rig, 2026-11-26, applet 7.2.0 — all pass:
//! - a session authenticated with the phone's key but without secure
//!   messaging cannot sign (CommandNotAllowedNoEf): the key's policy
//!   (ALLOW_SIGN | REQUIRE_SM for the phone credential) is enforced;
//! - the phone's protected sign command (full level: C-MAC, C-DEC,
//!   R-MAC, R-ENC), relayed unchanged, signs; the response MAC checks and
//!   the signature verifies with ed25519-dalek;
//! - the same command replayed, or with one protected byte flipped, is
//!   refused (SecurityStatusNotSatisfied): the MAC chain binds each
//!   command — key id and digest included — to its one place in the
//!   session; the digest travels encrypted;
//! - a static UserID session cannot sign;
//! - inside the protected session, plain commands are refused; protected
//!   commands work at C-MAC and at the full level, short or extended.
//!
//! What got it working: inside the applet session wrapper the protected
//! command ends with its MAC — no Le — as in NXP's nano-package
//! (Se05x_API_ECKeyAuth_Encrypt). With an Le byte after the MAC every
//! protected command was refused.
//!
//! Not shown here: that the chip refuses the phone factor alone (it does
//! not — a holder of the session keys can sign; local confirmation and
//! the PIN are the controller's), and combining the phone's session with
//! a chip-enforced PIN.

#[cfg(target_os = "linux")]
fn main() {
    use se05x::se05x::commands::{
        CheckObjectExists, CloseSession, CreateSession, DeleteSecureObject, EddsaSign,
        EddsaSignResponse, ScpInitializeUpdate, VerifySessionUserId, WriteEcKey, WriteSymmKey,
        WriteUserId,
    };
    use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags as F, Policy, PolicySet};
    use se05x::se05x::{EcCurve, ObjectId, P1KeyType, Se05XResponse, SessionId, SymmKeyType};
    use signer_pi::scp03::{split, Session, C_MAC, FULL};
    use signer_pi::se051::{from_chip, verify, Se051};
    use signer_pi::se051_raw::{bytes, Apdu};

    const CLEANER: ObjectId = ObjectId([0x7D, 0xA0, 0x20, 0x01]);
    const PHONE: ObjectId = ObjectId([0x7D, 0xA0, 0x20, 0x02]);
    const KEY: ObjectId = ObjectId([0x7D, 0xA0, 0x20, 0x10]);
    const CLEANER_SECRET: &[u8] = b"cleaner-test-01";
    const DIGEST: [u8; 32] = [0x42; 32];

    let failures = std::cell::Cell::new(0);
    let check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures.set(failures.get() + 1);
        }
    };
    let stop = |msg: String| -> ! {
        eprintln!("STOP {msg}");
        std::process::exit(2)
    };

    let mut se = Se051::open("/dev/i2c-1").expect("open the SE051");
    let chip = se.chip();
    let exists = |chip: &mut se05x::se05x::Se05X<_, _>, id: ObjectId| {
        let chip: &mut se05x::se05x::Se05X<_, _> = chip;
        chip.run_command(&CheckObjectExists { object_id: id }, &mut [0u8; 16])
            .map(|r| r.result.is_success())
            .unwrap_or(true)
    };

    // The cleaner: a UserID credential allowed to delete itself.
    let self_delete = [Policy {
        object_id: CLEANER,
        access_rule: ObjectAccessRule::from_flags(F::ALLOW_DELETE),
    }];
    let write_cleaner = |chip: &mut se05x::se05x::Se05X<_, _>| {
        let chip: &mut se05x::se05x::Se05X<_, _> = chip;
        chip.run_command(
            &WriteUserId {
                policy: Some(PolicySet(&self_delete)),
                max_attempts: None,
                object_id: CLEANER,
                data: CLEANER_SECRET,
            },
            &mut [0u8; 16],
        )
        .map(|_| ())
    };
    let cleaner_session =
        |chip: &mut se05x::se05x::Se05X<_, _>| -> Result<SessionId, se05x::se05x::Error> {
            let chip: &mut se05x::se05x::Se05X<_, _> = chip;
            let id = chip
                .run_command(&CreateSession { object_id: CLEANER }, &mut [0u8; 64])?
                .session_id;
            chip.run_session_command(
                id,
                &VerifySessionUserId {
                    user_id: CLEANER_SECRET,
                },
                &mut [0u8; 16],
            )?;
            Ok(id)
        };
    if std::env::args().nth(1).as_deref() == Some("--cleanup") {
        let id = cleaner_session(chip).unwrap_or_else(|e| stop(format!("cleaner session: {e:?}")));
        for obj in [KEY, PHONE, CLEANER] {
            let r = chip
                .run_session_command(id, &DeleteSecureObject { object_id: obj }, &mut [0u8; 16])
                .map(|_| ());
            println!("delete {obj:?}: {r:?}, exists now: {}", exists(chip, obj));
        }
        return;
    }
    for id in [CLEANER, PHONE, KEY] {
        if exists(chip, id) {
            stop(format!(
                "scratch object {id:?} exists; not touching anything"
            ));
        }
    }
    if let Err(e) = write_cleaner(chip) {
        stop(format!("write the cleaner: {e:?}"));
    }
    let proven = cleaner_session(chip).and_then(|id| {
        chip.run_session_command(
            id,
            &DeleteSecureObject { object_id: CLEANER },
            &mut [0u8; 16],
        )
        .map(|_| ())
    });
    let gone = !exists(chip, CLEANER);
    check(
        "a credential with a self-delete policy deletes itself",
        proven.is_ok() && gone,
        format!("{proven:?}"),
    );
    if !gone {
        stop("the cleaner cannot delete itself; creating nothing else".into());
    }
    if let Err(e) = write_cleaner(chip) {
        stop(format!("write the cleaner again: {e:?}"));
    }

    // The phone's credential: an AES key the cleaner may delete.
    let phone_key: [u8; 16] = rand::random();
    let by_cleaner = [Policy {
        object_id: CLEANER,
        access_rule: ObjectAccessRule::from_flags(F::ALLOW_DELETE),
    }];
    let r = chip.run_command(
        &WriteSymmKey {
            transient: false,
            is_auth: true,
            key_type: SymmKeyType::Aes,
            policy: Some(PolicySet(&by_cleaner)),
            max_attempts: None,
            object_id: PHONE,
            kek_id: None,
            value: &phone_key,
        },
        &mut [0u8; 16],
    );
    check("write the phone credential", r.is_ok(), format!("{r:?}"));

    // The disposable key: signs only for the phone with secure messaging.
    let seed: [u8; 32] = rand::random();
    let public = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();
    let key_policy = [
        Policy {
            object_id: PHONE,
            access_rule: ObjectAccessRule::from_flags(F::ALLOW_SIGN.union(F::REQUIRE_SM)),
        },
        Policy {
            object_id: CLEANER,
            access_rule: ObjectAccessRule::from_flags(F::ALLOW_DELETE),
        },
    ];
    let r = chip.run_command(
        &WriteEcKey {
            transient: false,
            is_auth: false,
            key_type: Some(P1KeyType::KeyPair),
            policy: Some(PolicySet(&key_policy)),
            max_attempts: None,
            object_id: KEY,
            curve: Some(EcCurve::IdEccEd25519),
            private_key: Some(&seed),
            public_key: Some(&public),
        },
        &mut [0u8; 16],
    );
    check("write the disposable key", r.is_ok(), format!("{r:?}"));

    if failures.get() == 0 {
        // The phone authenticates a session at `level`; the controller relays.
        let open = |chip: &mut se05x::se05x::Se05X<_, _>,
                    level: u8|
         -> Result<(SessionId, Session), String> {
            let chip: &mut se05x::se05x::Se05X<_, _> = chip;
            let id = chip
                .run_command(&CreateSession { object_id: PHONE }, &mut [0u8; 64])
                .map_err(|e| format!("create: {e:?}"))?
                .session_id;
            let host_challenge: [u8; 8] = rand::random();
            let init = chip
                .run_session_command(id, &ScpInitializeUpdate { host_challenge }, &mut [0u8; 64])
                .map_err(|e| format!("initialize update: {e:?}"))?
                .se05x_challenge;
            let auth = Session::authenticate(
                &phone_key,
                &host_challenge,
                &init.card_challenge,
                &init.card_cryptogram,
                level,
            )
            .map_err(|e| format!("{e:?}"))?;
            chip.run_session_command(id, &Apdu(auth.apdu), &mut [0u8; 16])
                .map_err(|e| format!("external authenticate: {e:?}"))?;
            Ok((id, auth.session))
        };
        let sign_apdu = |digest: &[u8; 32]| {
            bytes(&EddsaSign {
                key_id: KEY,
                data: digest,
            })
        };
        let phone_signs = |session: &mut Session, digest: &[u8; 32]| -> Option<Vec<u8>> {
            let plain = sign_apdu(digest);
            let (header, data, _) = split(&plain)?;
            Some(session.wrap(&header, data, None))
        };
        let signature = |reply: &[u8]| -> bool {
            EddsaSignResponse::from_response(reply)
                .ok()
                .and_then(|r| from_chip(r.signature).ok())
                .is_some_and(|s| verify(&public, &DIGEST, &s).is_ok())
        };

        // 1. An authenticated session without MACs cannot sign.
        match open(chip, 0x00) {
            Ok((id, _)) => {
                let r = chip
                    .run_session_command(id, &Apdu(sign_apdu(&DIGEST)), &mut [0u8; 256])
                    .map(|r| r.0.to_vec());
                check(
                    "phone session without MACs cannot sign",
                    r.is_err(),
                    format!("{r:?}"),
                );
                let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            }
            Err(e) => check(
                "phone session without MACs cannot sign",
                true,
                format!("session refused: {e}"),
            ),
        }

        // Diagnostic: a harmless protected command at each level tells a
        // wrong MAC apart from a level the chip does not accept.
        for (name, level) in [("C-MAC", C_MAC), ("full", FULL)] {
            match open(chip, level) {
                Ok((id, mut session)) => {
                    let plain = bytes(&se05x::se05x::commands::GetRandom { length: 8.into() });
                    let apdu = split(&plain)
                        .map(|(h, d, _)| session.wrap(&h, d, None))
                        .unwrap_or_default();
                    let r = chip
                        .run_session_command(id, &Apdu(apdu), &mut [0u8; 64])
                        .map_err(|e| format!("{e:?}"))
                        .and_then(|r| session.unwrap(r.0).map_err(|e| format!("{e:?}")))
                        .map(|d| d.len());
                    println!("INFO protected GetRandom, {name} session: {r:?}");
                    let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
                }
                Err(e) => println!("INFO {name} session: {e}"),
            }
        }

        // Probes: a plain command in the authenticated session, and the
        // protected command in extended-length form.
        if let Ok((id, _)) = open(chip, C_MAC) {
            let plain = bytes(&se05x::se05x::commands::GetRandom { length: 8.into() });
            println!("INFO driver's GetRandom bytes: {}", hex::encode(&plain));
            let r = chip
                .run_session_command(id, &Apdu(plain), &mut [0u8; 64])
                .map(|r| r.0.len());
            println!("INFO plain GetRandom in a C-MAC session: {r:?}");
            let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
        }
        for (name, level) in [("C-MAC", C_MAC), ("full", FULL)] {
            if let Ok((id, mut session)) = open(chip, level) {
                let plain = bytes(&se05x::se05x::commands::GetRandom { length: 8.into() });
                let apdu = split(&plain)
                    .map(|(h, d, _)| session.wrap_extended(&h, d, None))
                    .unwrap_or_default();
                let r = chip
                    .run_session_command(id, &Apdu(apdu), &mut [0u8; 64])
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|r| session.unwrap(r.0).map_err(|e| format!("{e:?}")))
                    .map(|d| d.len());
                println!("INFO extended protected GetRandom, {name} session: {r:?}");
                let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            }
        }

        // 2. The phone's protected command signs; 3. its replay is refused.
        match open(chip, FULL) {
            Ok((id, mut session)) => {
                let apdu = phone_signs(&mut session, &DIGEST).unwrap_or_default();
                let r = chip
                    .run_session_command(id, &Apdu(apdu.clone()), &mut [0u8; 256])
                    .map(|r| session.unwrap(r.0).is_ok_and(|d| signature(&d)));
                check(
                    "the phone's command signs, and it verifies",
                    matches!(r, Ok(true)),
                    format!("{r:?}"),
                );
                let r = chip
                    .run_session_command(id, &Apdu(apdu), &mut [0u8; 256])
                    .map(|r| r.0.to_vec());
                check("a replay of it is refused", r.is_err(), format!("{r:?}"));
                let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            }
            Err(e) => check("the phone's command signs, and it verifies", false, e),
        }

        // 4. The controller swaps bytes under the phone's MAC.
        match open(chip, FULL) {
            Ok((id, mut session)) => {
                let mut apdu = phone_signs(&mut session, &DIGEST).unwrap_or_default();
                // The digest travels encrypted: the controller cannot swap
                // it, only alter the protected bytes. Flip one.
                if let Some(byte) = apdu.get_mut(5) {
                    *byte ^= 0x01;
                }
                let r = chip
                    .run_session_command(id, &Apdu(apdu), &mut [0u8; 256])
                    .map(|r| r.0.to_vec());
                check(
                    "altered bytes under the phone's MAC are refused",
                    r.is_err(),
                    format!("{r:?}"),
                );
                let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            }
            Err(e) => check("altered bytes under the phone's MAC are refused", false, e),
        }

        // 5. The static UserID credential cannot sign.
        match cleaner_session(chip) {
            Ok(id) => {
                let r = chip
                    .run_session_command(
                        id,
                        &EddsaSign {
                            key_id: KEY,
                            data: &DIGEST,
                        },
                        &mut [0u8; 256],
                    )
                    .map(|r| r.signature.to_vec());
                check("a UserID session cannot sign", r.is_err(), format!("{r:?}"));
                let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            }
            Err(e) => check("a UserID session cannot sign", false, format!("{e:?}")),
        }
    }

    // Cleanup through the cleaner, then the cleaner deletes itself.
    match cleaner_session(chip) {
        Ok(id) => {
            for obj in [KEY, PHONE, CLEANER] {
                let _ = chip.run_session_command(
                    id,
                    &DeleteSecureObject { object_id: obj },
                    &mut [0u8; 16],
                );
            }
        }
        Err(e) => eprintln!("cleaner session: {e:?}"),
    }
    for id in [KEY, PHONE, CLEANER] {
        let left = exists(chip, id);
        check("cleanup", !left, format!("{id:?} exists={left}"));
    }
    std::process::exit(if failures.get() == 0 { 0 } else { 1 });
}

#[cfg(not(target_os = "linux"))]
fn main() {}

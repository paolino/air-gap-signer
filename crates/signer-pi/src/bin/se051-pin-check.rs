//! Experiment C for the secure project definition: a chip-enforced PIN
//! retry limit, and a signature that needs both the PIN and the phone.
//!
//! 1. A UserID PIN with a retry limit of 2, written with the limit as two
//!    bytes (the driver's one-byte form is refused): the right PIN works,
//!    then two wrong attempts block the right one too.
//! 2. A PCR (hash register) only a PIN session may extend. The key's
//!    policy lets the phone's protected (SCP03) session sign only while
//!    the PCR holds the value one PIN-session extension produces. Checked:
//!    phone before PIN, extension without a session, PIN then phone, the
//!    PCR moved on, PIN alone.
//!
//! Scratch range 7DA030xx; a cleaner credential deletes everything,
//! itself last. Refuses to start if any scratch object exists; exits
//! non-zero on any unexpected answer; `--cleanup` removes leftovers.
//!
//! Result on the rig, 2026-11-26, applet 7.2.0 — all pass:
//! - the retry limit as two bytes is accepted (the driver's one-byte
//!   form is refused with IncorrectDataParameter); after two wrong PINs
//!   the right one is refused (CommandNotAllowedNoEf);
//! - phone before PIN, the PIN alone, and an extension without a session
//!   are refused; PIN-session extension then the phone's protected
//!   command signs, and the signature verifies; once the PCR moves on the
//!   phone is refused again;
//! - a PCR is reset by a write with neither initial value nor extension
//!   (rewriting the initial value is refused); one extension does not
//!   give sha256(initial || data), so the target is read from the chip.
//!
//! Limits: between the PIN's extension and the PCR moving on, the phone
//! alone can sign — moving it on after each signature is the
//! controller's job. The UserID PIN crosses the bus in clear (on the
//! front board that bus belongs to the controller). Lock persistence
//! across a power cycle is not tested here.

#[cfg(target_os = "linux")]
fn main() {
    use se05x::se05x::commands::{
        CheckObjectExists, CloseSession, CreateSession, DeleteSecureObject, EddsaSign,
        EddsaSignResponse, ReadObject, ScpInitializeUpdate, VerifySessionUserId, WriteEcKey,
        WritePcr, WriteSymmKey, WriteUserId,
    };
    use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags as F, Policy, PolicySet};
    use se05x::se05x::{EcCurve, ObjectId, P1KeyType, Se05XResponse, SessionId, SymmKeyType};
    use sha2::{Digest, Sha256};
    use signer_pi::scp03::{split, Session, FULL};
    use signer_pi::se051::{from_chip, verify, Se051};
    use signer_pi::se051_raw::{bytes, command, policy, tlv, Apdu};

    const CLEANER: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x01]);
    const RETRY: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x02]);
    const PIN: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x03]);
    const PHONE: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x04]);
    const PCR: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x05]);
    const KEY: ObjectId = ObjectId([0x7D, 0xA0, 0x30, 0x10]);
    const ALL: [ObjectId; 6] = [KEY, PCR, PHONE, PIN, RETRY, CLEANER];
    const CLEANER_SECRET: &[u8] = b"cleaner-test-03";
    const PIN_SECRET: &[u8] = b"pin-test-0003";
    const PIN_OK: &[u8] = b"pin verified";
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
    type Chip<'c> = &'c mut se05x::se05x::Se05X<
        se05x::embedded_hal::Hal10<signer_pi::se051::Bus>,
        se05x::embedded_hal::Hal10<linux_embedded_hal::Delay>,
    >;
    fn exists(chip: Chip, id: ObjectId) -> bool {
        chip.run_command(&CheckObjectExists { object_id: id }, &mut [0u8; 16])
            .map(|r| r.result.is_success())
            .unwrap_or(true)
    }
    fn user_session(
        chip: Chip,
        cred: ObjectId,
        secret: &[u8],
    ) -> Result<SessionId, se05x::se05x::Error> {
        let id = chip
            .run_command(&CreateSession { object_id: cred }, &mut [0u8; 64])?
            .session_id;
        chip.run_session_command(id, &VerifySessionUserId { user_id: secret }, &mut [0u8; 16])?;
        Ok(id)
    }
    fn close(chip: Chip, id: SessionId) {
        let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
    }
    let cleanup = |chip: Chip| match user_session(chip, CLEANER, CLEANER_SECRET) {
        Ok(id) => {
            for obj in ALL {
                let _ = chip.run_session_command(
                    id,
                    &DeleteSecureObject { object_id: obj },
                    &mut [0u8; 16],
                );
            }
        }
        Err(e) => eprintln!("cleaner session: {e:?}"),
    };

    if std::env::args().nth(1).as_deref() == Some("--cleanup") {
        cleanup(chip);
        for id in ALL {
            println!("{id:?} exists: {}", exists(chip, id));
        }
        return;
    }
    for id in ALL {
        if exists(chip, id) {
            stop(format!("scratch object {id:?} exists; run --cleanup"));
        }
    }

    // The cleaner deletes itself and every other scratch object.
    let self_delete = [Policy {
        object_id: CLEANER,
        access_rule: ObjectAccessRule::from_flags(F::ALLOW_DELETE),
    }];
    if let Err(e) = chip.run_command(
        &WriteUserId {
            policy: Some(PolicySet(&self_delete)),
            max_attempts: None,
            object_id: CLEANER,
            data: CLEANER_SECRET,
        },
        &mut [0u8; 16],
    ) {
        stop(format!("write the cleaner: {e:?}"));
    }
    let by_cleaner = policy(CLEANER.0, F::ALLOW_DELETE.bits(), None);

    // A UserID with a two-byte retry limit, built by hand.
    let write_user = |chip: Chip, id: ObjectId, secret: &[u8], attempts: u16| {
        let header: [u8; 4] = bytes(&WriteUserId {
            policy: None,
            max_attempts: None,
            object_id: id,
            data: secret,
        })[..4]
            .try_into()
            .unwrap();
        let data = [
            tlv(0x11, &by_cleaner),
            tlv(0x12, &attempts.to_be_bytes()),
            tlv(0x41, &id.0),
            tlv(0x42, secret),
        ]
        .concat();
        chip.run_command(&Apdu(command(header, &data, None)), &mut [0u8; 16])
            .map(|_| ())
    };

    // 1. The retry limit.
    let r = write_user(chip, RETRY, PIN_SECRET, 2);
    check(
        "a UserID with a two-byte retry limit is accepted",
        r.is_ok(),
        format!("{r:?}"),
    );
    if r.is_ok() {
        let attempt = |chip: Chip, secret: &[u8]| {
            let r = user_session(chip, RETRY, secret);
            if let Ok(id) = r {
                close(chip, id);
            }
            r.map(|_| ())
        };
        let r = attempt(chip, PIN_SECRET);
        check("the right PIN opens it", r.is_ok(), format!("{r:?}"));
        let w1 = attempt(chip, b"wrong-pin-001");
        let w2 = attempt(chip, b"wrong-pin-002");
        check(
            "wrong PINs are refused",
            w1.is_err() && w2.is_err(),
            format!("{w1:?} {w2:?}"),
        );
        let r = attempt(chip, PIN_SECRET);
        check(
            "after two wrong PINs the right one is refused too",
            r.is_err(),
            format!("{r:?}"),
        );
    }

    // 2. PIN and phone.
    let r = write_user(chip, PIN, PIN_SECRET, 3);
    check("write the PIN", r.is_ok(), format!("{r:?}"));
    let phone_key: [u8; 16] = rand::random();
    let r = chip.run_command(
        &WriteSymmKey {
            transient: false,
            is_auth: true,
            key_type: SymmKeyType::Aes,
            policy: Some(PolicySet(&[Policy {
                object_id: CLEANER,
                access_rule: ObjectAccessRule::from_flags(F::ALLOW_DELETE),
            }])),
            max_attempts: None,
            object_id: PHONE,
            kek_id: None,
            value: &phone_key,
        },
        &mut [0u8; 16],
    );
    check("write the phone credential", r.is_ok(), format!("{r:?}"));
    let pcr_policy = [
        Policy {
            object_id: PIN,
            access_rule: ObjectAccessRule::from_flags(F::ALLOW_WRITE.union(F::ALLOW_READ)),
        },
        Policy {
            object_id: CLEANER,
            access_rule: ObjectAccessRule::from_flags(
                F::ALLOW_DELETE.union(F::ALLOW_WRITE).union(F::ALLOW_READ),
            ),
        },
    ];
    let initial = [0u8; 32];
    let r = chip.run_command(
        &WritePcr {
            transient: false,
            policy: Some(PolicySet(&pcr_policy)),
            object_id: PCR,
            initial_value: Some(&initial),
            extend: None,
        },
        &mut [0u8; 16],
    );
    check("write the PCR", r.is_ok(), format!("{r:?}"));

    let extend = |chip: Chip, session: Option<SessionId>, data: &[u8]| {
        let cmd = WritePcr {
            transient: false,
            policy: None,
            object_id: PCR,
            initial_value: None,
            extend: Some(data),
        };
        match session {
            Some(id) => chip
                .run_session_command(id, &cmd, &mut [0u8; 16])
                .map(|_| ()),
            None => chip.run_command(&cmd, &mut [0u8; 16]).map(|_| ()),
        }
    };
    let read_pcr = |chip: Chip, id: SessionId| {
        chip.run_session_command(
            id,
            &ReadObject {
                object_id: PCR,
                offset: None,
                length: None,
                rsa_key_component: None,
            },
            &mut [0u8; 64],
        )
        .map(|r| r.data.to_vec())
    };
    let reset = |chip: Chip| -> Result<(), String> {
        let id = user_session(chip, CLEANER, CLEANER_SECRET).map_err(|e| format!("{e:?}"))?;
        let r = chip
            .run_session_command(
                id,
                &WritePcr {
                    transient: false,
                    policy: None,
                    object_id: PCR,
                    initial_value: None,
                    extend: None,
                },
                &mut [0u8; 16],
            )
            .map(|_| ())
            .map_err(|e| format!("{e:?}"));
        close(chip, id);
        r
    };

    // Reset: a write with neither an initial value nor an extension.
    // The value one PIN-session extension produces, read from the chip.
    let target = match user_session(chip, PIN, PIN_SECRET) {
        Ok(id) => {
            let r = extend(chip, Some(id), PIN_OK)
                .map_err(|e| format!("{e:?}"))
                .and_then(|_| read_pcr(chip, id).map_err(|e| format!("{e:?}")));
            close(chip, id);
            r
        }
        Err(e) => Err(format!("{e:?}")),
    };
    let target: [u8; 32] = match target.as_deref().map(<[u8; 32]>::try_from) {
        Ok(Ok(v)) => v,
        other => {
            check(
                "a PIN session extends the PCR and reads it",
                false,
                format!("{other:?}"),
            );
            cleanup(chip);
            std::process::exit(1);
        }
    };
    let host = Sha256::new()
        .chain_update(initial)
        .chain_update(PIN_OK)
        .finalize();
    println!(
        "INFO PCR after one extension equals sha256(initial || data): {}",
        host[..] == target
    );
    let r = reset(chip);
    check("the cleaner resets the PCR", r.is_ok(), format!("{r:?}"));

    // The key: the phone's protected session signs, only at that PCR value.
    let seed: [u8; 32] = rand::random();
    let public = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();
    let key_policy = [
        policy(
            PHONE.0,
            F::ALLOW_SIGN.union(F::REQUIRE_SM).bits(),
            Some((PCR.0, target)),
        ),
        by_cleaner.clone(),
    ]
    .concat();
    let plain = bytes(&WriteEcKey {
        transient: false,
        is_auth: false,
        key_type: Some(P1KeyType::KeyPair),
        policy: None,
        max_attempts: None,
        object_id: KEY,
        curve: Some(EcCurve::IdEccEd25519),
        private_key: Some(&seed),
        public_key: Some(&public),
    });
    let r = match split(&plain) {
        Some((header, data, _)) => {
            let data = [tlv(0x11, &key_policy), data.to_vec()].concat();
            chip.run_command(&Apdu(command(header, &data, None)), &mut [0u8; 16])
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        }
        None => Err("cannot parse the driver's WriteEcKey".into()),
    };
    check(
        "write the key with the PCR requirement",
        r.is_ok(),
        format!("{r:?}"),
    );

    // The phone opens a protected session and signs (one command).
    let phone_signs = |chip: Chip| -> Result<bool, String> {
        let id = chip
            .run_command(&CreateSession { object_id: PHONE }, &mut [0u8; 64])
            .map_err(|e| format!("{e:?}"))?
            .session_id;
        let host_challenge: [u8; 8] = rand::random();
        let init = chip
            .run_session_command(id, &ScpInitializeUpdate { host_challenge }, &mut [0u8; 64])
            .map_err(|e| format!("{e:?}"))?
            .se05x_challenge;
        let auth = Session::authenticate(
            &phone_key,
            &host_challenge,
            &init.card_challenge,
            &init.card_cryptogram,
            FULL,
        )
        .map_err(|e| format!("{e:?}"))?;
        let mut session = auth.session;
        chip.run_session_command(id, &Apdu(auth.apdu), &mut [0u8; 16])
            .map_err(|e| format!("{e:?}"))?;
        let plain = bytes(&EddsaSign {
            key_id: KEY,
            data: &DIGEST,
        });
        let (header, data, _) = split(&plain).ok_or("cannot parse the sign command")?;
        let apdu = session.wrap(&header, data, None);
        let r = chip
            .run_session_command(id, &Apdu(apdu), &mut [0u8; 256])
            .map(|r| r.0.to_vec());
        close(chip, id);
        let reply = r.map_err(|e| format!("{e:?}"))?;
        let data = session.unwrap(&reply).map_err(|e| format!("{e:?}"))?;
        Ok(EddsaSignResponse::from_response(&data)
            .ok()
            .and_then(|r| from_chip(r.signature).ok())
            .is_some_and(|s| verify(&public, &DIGEST, &s).is_ok()))
    };

    if failures.get() == 0 {
        let r = phone_signs(chip);
        check(
            "the phone alone, before the PIN, cannot sign",
            r.is_err(),
            format!("{r:?}"),
        );

        let r = extend(chip, None, PIN_OK);
        check(
            "no session cannot extend the PCR",
            r.is_err(),
            format!("{r:?}"),
        );

        match user_session(chip, PIN, PIN_SECRET) {
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
                check("the PIN alone cannot sign", r.is_err(), format!("{r:?}"));
                let r = extend(chip, Some(id), PIN_OK);
                check("a PIN session extends the PCR", r.is_ok(), format!("{r:?}"));
                close(chip, id);
            }
            Err(e) => check("open a PIN session", false, format!("{e:?}")),
        }
        let r = phone_signs(chip);
        check(
            "PIN then phone: signs, and it verifies",
            matches!(r, Ok(true)),
            format!("{r:?}"),
        );

        match user_session(chip, PIN, PIN_SECRET) {
            Ok(id) => {
                let r = extend(chip, Some(id), b"consumed");
                check("the PCR moves on", r.is_ok(), format!("{r:?}"));
                close(chip, id);
            }
            Err(e) => check("open a PIN session", false, format!("{e:?}")),
        }
        let r = phone_signs(chip);
        check(
            "after the PCR moved on, the phone cannot sign",
            r.is_err(),
            format!("{r:?}"),
        );
    }

    cleanup(chip);
    for id in ALL {
        let left = exists(chip, id);
        check("cleanup", !left, format!("{id:?} exists={left}"));
    }
    std::process::exit(if failures.get() == 0 { 0 } else { 1 });
}

#[cfg(not(target_os = "linux"))]
fn main() {}

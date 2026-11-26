//! Experiment A for the secure project definition ("Where the phone
//! factor is enforced"): can the SE051 itself confine signing to a
//! session authenticated with one credential?
//!
//! Creates, in a scratch range of object ids, two UserID credentials —
//! "pin" and "phone" — and a disposable Ed25519 key whose policy grants
//! signing and deletion to the phone credential only. Records the chip's
//! answer with no session, a pin session, a phone session and a phone
//! session with the wrong secret, then removes everything it created.
//! Refuses to start if any scratch object exists. Exits non-zero if the
//! chip does not behave as the policy says.
//!
//! Result on the rig, 2026-11-26, applet 7.2.0:
//! - no session: sign and delete refused (CommandNotAllowedNoEf);
//! - pin session (correct secret): sign refused;
//! - phone credential, wrong secret: verify refused
//!   (ConditionsOfUseNotSatisfied), sign refused;
//! - phone session: signs, the signature verifies (ed25519-dalek), and
//!   it can delete the key.
//!
//! So the chip itself confines signing to one credential. Two findings:
//! - a UserID credential written with the default policy cannot be
//!   deleted afterwards — not with no session, not from its own session,
//!   not from the other credential's session (CommandNotAllowedNoEf).
//!   The first run's two credentials (7DA01001, 7DA01002) remain on the
//!   rig's chip; this check therefore no longer starts there. Credentials
//!   must be written with a policy that allows their deletion.
//! - `max_attempts` on WriteUserId is refused (IncorrectDataParameter):
//!   the driver encodes it as one byte. The credentials were written
//!   without a retry limit. A chip-enforced retry limit needs that field
//!   encoded as the applet expects.
//!
//! A UserID secret travels in clear over I2C and can be replayed by
//! anything on the bus; binding a signature to one digest needs an
//! AES-key session with per-command MACs (experiment B).

#[cfg(target_os = "linux")]
fn main() {
    use se05x::se05x::commands::{
        CheckObjectExists, CloseSession, CreateSession, DeleteSecureObject, EddsaSign,
        VerifySessionUserId, WriteEcKey, WriteUserId,
    };
    use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags, Policy, PolicySet};
    use se05x::se05x::{EcCurve, ObjectId, P1KeyType};
    use signer_pi::se051::{from_chip, verify, Se051};

    const PIN: ObjectId = ObjectId([0x7D, 0xA0, 0x10, 0x01]);
    const PHONE: ObjectId = ObjectId([0x7D, 0xA0, 0x10, 0x02]);
    const KEY: ObjectId = ObjectId([0x7D, 0xA0, 0x10, 0x10]);
    const PIN_SECRET: &[u8] = b"pin-test-0001";
    const PHONE_SECRET: &[u8] = b"phone-test-0001";
    const DIGEST: [u8; 32] = [0x42; 32];

    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    let mut se = Se051::open("/dev/i2c-1").expect("open the SE051");
    println!("applet: {}", se.version().expect("version"));
    let chip = se.chip();
    let buf = &mut [0u8; 512];

    // A credential can only be deleted from a session authenticated with
    // itself: each one deletes itself.
    let delete_credential =
        |chip: &mut se05x::se05x::Se05X<_, _>, cred: ObjectId, secret: &[u8]| {
            let chip: &mut se05x::se05x::Se05X<_, _> = chip;
            let id = chip
                .run_command(&CreateSession { object_id: cred }, &mut [0u8; 64])
                .inspect_err(|e| eprintln!("  create session: {e:?}"))?
                .session_id;
            chip.run_session_command(id, &VerifySessionUserId { user_id: secret }, &mut [0u8; 64])
                .inspect_err(|e| eprintln!("  verify: {e:?}"))?;
            chip.run_session_command(id, &DeleteSecureObject { object_id: cred }, &mut [0u8; 16])
                .inspect_err(|e| eprintln!("  delete in session: {e:?}"))?;
            let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            Ok::<(), se05x::se05x::Error>(())
        };
    if std::env::args().nth(1).as_deref() == Some("--cross-delete") {
        // Each credential deleted from a session of the other one.
        for (target, via, secret) in [(PIN, PHONE, PHONE_SECRET), (PHONE, PIN, PIN_SECRET)] {
            let id = chip
                .run_command(&CreateSession { object_id: via }, &mut [0u8; 64])
                .expect("session")
                .session_id;
            chip.run_session_command(id, &VerifySessionUserId { user_id: secret }, &mut [0u8; 64])
                .expect("verify");
            let r = chip
                .run_session_command(
                    id,
                    &DeleteSecureObject { object_id: target },
                    &mut [0u8; 16],
                )
                .map(|_| ());
            let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
            let left = chip
                .run_command(&CheckObjectExists { object_id: target }, &mut [0u8; 16])
                .map(|r| r.result.is_success());
            println!("delete {target:?} via {via:?}: {r:?}, exists now: {left:?}");
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--cleanup") {
        for (cred, secret) in [(PIN, PIN_SECRET), (PHONE, PHONE_SECRET)] {
            let r = delete_credential(chip, cred, secret);
            let left = chip
                .run_command(&CheckObjectExists { object_id: cred }, &mut [0u8; 16])
                .map(|r| r.result.is_success());
            println!("delete {cred:?}: {r:?}, exists now: {left:?}");
        }
        return;
    }
    for id in [PIN, PHONE, KEY] {
        let exists = chip
            .run_command(&CheckObjectExists { object_id: id }, buf)
            .expect("check object")
            .result
            .is_success();
        if exists {
            eprintln!("scratch object {id:?} exists; not touching anything");
            std::process::exit(1);
        }
    }

    // The two credentials.
    for (id, secret) in [(PIN, PIN_SECRET), (PHONE, PHONE_SECRET)] {
        chip.run_command(
            &WriteUserId {
                policy: None,
                max_attempts: None,
                object_id: id,
                data: secret,
            },
            buf,
        )
        .expect("write credential");
    }

    // The disposable key: signing and deletion for the phone credential only.
    let seed: [u8; 32] = rand::random();
    let public = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();
    let policy = [Policy {
        object_id: PHONE,
        access_rule: ObjectAccessRule::from_flags(
            ObjectPolicyFlags::ALLOW_SIGN.union(ObjectPolicyFlags::ALLOW_DELETE),
        ),
    }];
    chip.run_command(
        &WriteEcKey {
            transient: false,
            is_auth: false,
            key_type: Some(P1KeyType::KeyPair),
            policy: Some(PolicySet(&policy)),
            max_attempts: None,
            object_id: KEY,
            curve: Some(EcCurve::IdEccEd25519),
            private_key: Some(&seed),
            public_key: Some(&public),
        },
        buf,
    )
    .expect("write the disposable key");

    let sign = EddsaSign {
        key_id: KEY,
        data: &DIGEST,
    };

    // 1. No session.
    let r = chip.run_command(&sign, buf).map(|r| r.signature.to_vec());
    check("no session cannot sign", r.is_err(), format!("{r:?}"));

    // 2..4. Sessions with each credential.
    let session = |cred: ObjectId, secret: &[u8], chip: &mut _| {
        let chip: &mut se05x::se05x::Se05X<_, _> = chip;
        let id = chip
            .run_command(&CreateSession { object_id: cred }, &mut [0u8; 64])
            .expect("create session")
            .session_id;
        let verified =
            chip.run_session_command(id, &VerifySessionUserId { user_id: secret }, &mut [0u8; 64]);
        (id, verified.is_ok())
    };

    let (id, verified) = session(PIN, PIN_SECRET, chip);
    let r = chip
        .run_session_command(id, &sign, buf)
        .map(|r| r.signature.to_vec());
    check(
        "pin session cannot sign",
        verified && r.is_err(),
        format!("verified={verified} {r:?}"),
    );
    let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);

    let (id, verified) = session(PHONE, b"wrong-secret", chip);
    let r = chip
        .run_session_command(id, &sign, buf)
        .map(|r| r.signature.to_vec());
    check(
        "wrong phone secret cannot sign",
        !verified && r.is_err(),
        format!("verified={verified} {r:?}"),
    );
    let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);

    let (id, verified) = session(PHONE, PHONE_SECRET, chip);
    let r = chip
        .run_session_command(id, &sign, buf)
        .map(|r| r.signature.to_vec());
    let good = match &r {
        Ok(sig) => from_chip(sig)
            .and_then(|s| verify(&public, &DIGEST, &s))
            .is_ok(),
        Err(_) => false,
    };
    check(
        "phone session signs, and it verifies",
        verified && good,
        format!("verified={verified} ok={}", r.is_ok()),
    );

    // 5. Deletion without the phone session.
    let r = chip
        .run_command(&DeleteSecureObject { object_id: KEY }, &mut [0u8; 16])
        .map(|_| ());
    let still = chip
        .run_command(&CheckObjectExists { object_id: KEY }, &mut [0u8; 16])
        .map(|r| r.result.is_success());
    check(
        "no session cannot delete the key",
        r.is_err() && matches!(still, Ok(true)),
        format!("{r:?} still={still:?}"),
    );

    // Cleanup: the key through the phone session, then the credentials.
    let r = chip
        .run_session_command(id, &DeleteSecureObject { object_id: KEY }, &mut [0u8; 16])
        .map(|_| ());
    check("phone session deletes the key", r.is_ok(), format!("{r:?}"));
    let _ = chip.run_session_command(id, &CloseSession {}, &mut [0u8; 16]);
    for (cred, secret) in [(PIN, PIN_SECRET), (PHONE, PHONE_SECRET)] {
        let r = chip
            .run_command(&DeleteSecureObject { object_id: cred }, &mut [0u8; 16])
            .map(|_| ());
        check(
            "no session cannot delete a credential",
            r.is_err(),
            format!("{cred:?} {r:?}"),
        );
        let r = delete_credential(chip, cred, secret);
        check(
            "a credential deletes itself",
            r.is_ok(),
            format!("{cred:?} {r:?}"),
        );
    }
    for id in [PIN, PHONE, KEY] {
        let left = chip
            .run_command(&CheckObjectExists { object_id: id }, &mut [0u8; 16])
            .map(|r| r.result.is_success());
        check(
            "cleanup",
            matches!(left, Ok(false)),
            format!("{id:?} exists={left:?}"),
        );
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

#[cfg(not(target_os = "linux"))]
fn main() {}

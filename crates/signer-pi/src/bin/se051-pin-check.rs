//! Hardware acceptance with two reserved test objects and a public test PIN.
//! This program never accepts an object id, PIN, payment digest or wallet key as input.
#[cfg(target_os = "linux")]
mod bench {
    use ed25519_dalek::SigningKey;
    use se05x::se05x::commands::*;
    use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags as Flags, Policy, PolicySet};
    use se05x::se05x::{EcCurve, Error, ObjectId, P1KeyType, SessionId};
    use signer_pi::se051::{from_chip, verify, Se051};
    use std::fs::File;

    const AUTH: ObjectId = ObjectId([0x7d, 0xa0, 0x33, 0x01]);
    const KEY: ObjectId = ObjectId([0x7d, 0xa0, 0x33, 0x02]);
    const TEST_SEQUENCE: [u8; 9] = [1, 2, 4, 8, 1, 2, 4, 8, 1];
    type Result<T> = std::result::Result<T, String>;

    fn failure(e: Error) -> String {
        format!("{e:?}")
    }

    fn exists(se: &mut Se051, id: ObjectId) -> Result<bool> {
        Ok(se
            .chip()
            .run_command(&CheckObjectExists { object_id: id }, &mut [0; 64])
            .map_err(failure)?
            .result
            .is_success())
    }

    fn close(se: &mut Se051, session: SessionId) -> Result<()> {
        se.chip()
            .run_session_command(session, &CloseSession {}, &mut [0; 64])
            .map_err(failure)?;
        Ok(())
    }

    fn authenticate(se: &mut Se051) -> Result<SessionId> {
        let session = se
            .chip()
            .run_command(&CreateSession { object_id: AUTH }, &mut [0; 64])
            .map_err(failure)?
            .session_id;
        if let Err(e) = se.chip().run_session_command(
            session,
            &VerifySessionUserId {
                user_id: &TEST_SEQUENCE,
            },
            &mut [0; 64],
        ) {
            let _ = close(se, session);
            return Err(format!("authenticate trial PIN: {e:?}"));
        }
        Ok(session)
    }

    fn wrong_sequence(se: &mut Se051) -> Result<()> {
        let session = se
            .chip()
            .run_command(&CreateSession { object_id: AUTH }, &mut [0; 64])
            .map_err(failure)?
            .session_id;
        let mut wrong = TEST_SEQUENCE;
        wrong[0] = 8;
        let result = se.chip().run_session_command(
            session,
            &VerifySessionUserId { user_id: &wrong },
            &mut [0; 64],
        );
        let _ = close(se, session); // A failed verification can close the session itself.
        match result {
            Err(Error::Status(sw)) => {
                let code = u16::from(sw);
                if matches!(code, 0x6300 | 0x63c0..=0x63cf | 0x6982 | 0x6985 | 0x6986) {
                    println!("Incorrect trial sequence refused by chip ({code:04x})");
                    return Ok(());
                }
                Err(format!("unexpected PIN verification status {code:04x}"))
            }
            Err(e) => Err(format!("PIN verification I/O failure: {e:?}")),
            Ok(_) => Err("wrong sequence accepted".into()),
        }
    }

    fn gated(flags: Flags) -> Policy {
        Policy {
            object_id: AUTH,
            access_rule: ObjectAccessRule::from_flags(flags),
        }
    }

    fn require_refusal(se: &mut Se051, digest: &[u8; 32]) -> Result<()> {
        match se.chip().run_command(
            &EddsaSign {
                key_id: KEY,
                data: digest,
            },
            &mut [0; 256],
        ) {
            Err(Error::Status(sw)) if matches!(u16::from(sw), 0x6982 | 0x6985 | 0x6986) => {
                println!("Unverified signing refused by chip ({:04x})", u16::from(sw));
                Ok(())
            }
            Err(e) => Err(format!("expected an authorization refusal, got {e:?}")),
            Ok(_) => Err("signing outside a verified session succeeded".into()),
        }
    }

    pub fn run() -> Result<()> {
        let bus = File::open("/dev/i2c-1").map_err(|e| e.to_string())?;
        bus.try_lock().map_err(|e| e.to_string())?;
        let mut se = Se051::open("/dev/i2c-1").map_err(|e| e.to_string())?;
        if exists(&mut se, AUTH)? || exists(&mut se, KEY)? {
            return Err("reserved object occupied; stopped".into());
        }
        let pin_policy = [gated(Flags::ALLOW_DELETE)];
        let key_policy = [gated(Flags::ALLOW_SIGN | Flags::ALLOW_DELETE)];
        let buf = &mut [0; 256];
        let seed = se.fresh_seed().map_err(|e| e.to_string())?;
        let public = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        se.chip()
            .run_command(
                &signer_pi::se051::enroll::EnrollSequence::new(
                    AUTH,
                    &TEST_SEQUENCE,
                    PolicySet(&pin_policy),
                    5,
                )
                .ok_or("invalid test PIN policy")?,
                buf,
            )
            .map_err(|e| format!("create trial PIN: {e:?}"))?;
        let mut key_created = false;
        let trial = (|| -> Result<()> {
            se.chip()
                .run_command(
                    &WriteEcKey {
                        transient: false,
                        is_auth: false,
                        key_type: Some(P1KeyType::KeyPair),
                        policy: Some(PolicySet(&key_policy)),
                        max_attempts: None,
                        object_id: KEY,
                        curve: Some(EcCurve::IdEccEd25519),
                        private_key: Some(seed.as_slice()),
                        public_key: Some(&public),
                    },
                    buf,
                )
                .map_err(|e| format!("create protected key: {e:?}"))?;
            key_created = true;
            let digest = rand::random::<[u8; 32]>();
            require_refusal(&mut se, &digest)?;
            wrong_sequence(&mut se)?;
            let session = authenticate(&mut se)?;
            let signed = se
                .chip()
                .run_session_command(
                    session,
                    &EddsaSign {
                        key_id: KEY,
                        data: &digest,
                    },
                    buf,
                )
                .map_err(failure)
                .and_then(|r| from_chip(r.signature).map_err(|e| e.to_string()));
            close(&mut se, session)?;
            verify(&public, &digest, &signed?).map_err(|e| e.to_string())?;
            println!("Verified trial PIN session produced a valid Ed25519 signature");
            require_refusal(&mut se, &digest)?;
            Ok(())
        })();
        let cleanup = (|| -> Result<()> {
            let session = authenticate(&mut se)?;
            if key_created {
                se.chip()
                    .run_session_command(session, &DeleteSecureObject { object_id: KEY }, buf)
                    .map_err(failure)?;
            }
            se.chip()
                .run_session_command(session, &DeleteSecureObject { object_id: AUTH }, buf)
                .map_err(failure)?;
            // Deleting the authenticating object can itself close the session.
            let _ = close(&mut se, session);
            if exists(&mut se, KEY)? || exists(&mut se, AUTH)? {
                return Err("test objects still present".into());
            }
            println!("Both isolated objects verified absent");
            Ok(())
        })();
        match (trial, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (test, cleanup) => Err(format!("trial={test:?}, cleanup={cleanup:?}")),
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), String> {
    bench::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {}

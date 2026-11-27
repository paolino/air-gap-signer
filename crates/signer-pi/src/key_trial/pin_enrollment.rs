//! Migrate the practice object without spending or exporting a payment signature.
use super::*;
use crate::display::show_pin_count;
use panel_input::{EntryEvent, PinEntry, Sequence};
use se05x::se05x::commands::{
    CheckObjectExists, CloseSession, CreateSession, EddsaSign, VerifySessionUserId, WriteEcKey,
};
use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags as Flags, Policy, PolicySet};
use se05x::se05x::{EcCurve, P1KeyType, SessionId};

const AUTH: ObjectId = ObjectId([0x7d, 0xa0, 0x34, 1]);
const PROTECTED: ObjectId = ObjectId([0x7d, 0xa0, 0x34, 2]);
// Written and synced before the first chip mutation. Contains only the public key.
const RECORD: &str = "signing-pin-public.key";
const TRIES: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Phase {
    Legacy,
    Pending,
    Protected,
}

fn chip_error(e: se05x::se05x::Error) -> Error {
    format!("SE051: {e:?}").into()
}

fn exists(se: &mut Se051, id: ObjectId) -> Result<bool, Error> {
    Ok(se
        .chip()
        .run_command(&CheckObjectExists { object_id: id }, &mut [0; 64])
        .map_err(chip_error)?
        .result
        .is_success())
}

pub(super) fn inspect(se: &mut Se051, dir: &Path) -> Result<(Phase, Option<[u8; 32]>), Error> {
    let old = se.read_public_object(TRIAL_KEY)?;
    let auth = exists(se, AUTH)?;
    let protected = exists(se, PROTECTED)?;
    let record = match fs::read(dir.join(RECORD)) {
        Ok(bytes) => Some(
            bytes
                .try_into()
                .map_err(|_| "invalid PIN enrollment record")?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let state = classify(old, record, auth, protected)?;
    if let Some(public) = record {
        verify_export(dir, &public)?;
    }
    Ok((state, record.or(old)))
}

fn classify(
    old: Option<[u8; 32]>,
    record: Option<[u8; 32]>,
    auth: bool,
    protected: bool,
) -> Result<Phase, Error> {
    match (record, old, auth, protected) {
        (None, _, false, false) => Ok(Phase::Legacy),
        (Some(p), Some(o), a, k) if p == o && (!k || a) => Ok(Phase::Pending),
        (Some(_), None, true, true) => Ok(Phase::Protected),
        _ => Err("PIN enrollment records disagree with chip; stopped".into()),
    }
}

fn policy(flags: Flags) -> Policy {
    Policy {
        object_id: AUTH,
        access_rule: ObjectAccessRule::from_flags(flags),
    }
}

fn with_pin<T>(
    se: &mut Se051,
    pin: &Sequence,
    op: impl FnOnce(&mut Se051, SessionId) -> Result<T, Error>,
) -> Result<T, Error> {
    let id = se
        .chip()
        .run_command(&CreateSession { object_id: AUTH }, &mut [0; 64])
        .map_err(chip_error)?
        .session_id;
    let result = match se.chip().run_session_command(
        id,
        &VerifySessionUserId {
            user_id: pin.as_slice(),
        },
        &mut [0; 64],
    ) {
        Ok(_) => op(se, id),
        Err(_) => Err("PIN check failed. No automatic retry.".into()),
    };
    let closed = se
        .chip()
        .run_session_command(id, &CloseSession {}, &mut [0; 64])
        .map_err(chip_error);
    let value = result?;
    closed?;
    Ok(value)
}

fn prove(se: &mut Se051, pin: &Sequence, public: &[u8; 32]) -> Result<(), Error> {
    let challenge = rand::random::<[u8; 32]>();
    let sign = EddsaSign {
        key_id: PROTECTED,
        data: &challenge,
    };
    match se.chip().run_command(&sign, &mut [0; 256]) {
        Err(se05x::se05x::Error::Status(sw))
            if matches!(u16::from(sw), 0x6982 | 0x6985 | 0x6986) => {}
        _ => return Err("unverified chip session did not refuse signing".into()),
    }
    with_pin(se, pin, |se, id| {
        let mut buf = [0; 256];
        let reply = se
            .chip()
            .run_session_command(id, &sign, &mut buf)
            .map_err(chip_error)?;
        let sig = crate::se051::from_chip(reply.signature)?;
        Ok(crate::se051::verify(public, &challenge, &sig)?)
    })
}

// pin_contacts follows the order of GPIO LINES. PinEntry uses direction bits 0..3.
fn direction_mask(contacts: u8) -> u8 {
    const BITS: [u8; 6] = [6, 5, 0, 2, 1, 3]; // #5, #6, up, down, right, left
    BITS.iter()
        .enumerate()
        .fold(0, |mask, (i, bit)| mask | (((contacts >> i) & 1) << bit))
}

fn enter(
    hal: &mut (impl Display + recovery_pin::PinContacts),
    prompt: &str,
) -> Result<Option<Sequence>, Error> {
    show_pin_count(hal, prompt, 0)?;
    let mut entry = PinEntry::default();
    let mut previous = None;
    let mut since = std::time::Instant::now();
    loop {
        let contacts = direction_mask(hal.pin_contacts()?);
        if previous != Some(contacts) {
            previous = Some(contacts);
            since = std::time::Instant::now();
        }
        // Count a press and its release only after settling. Pass chords
        // immediately so even a brief overlap invalidates the movement.
        if contacts.count_ones() <= 1 && since.elapsed() < crate::buttons::DEBOUNCE {
            std::thread::sleep(crate::buttons::POLL);
            continue;
        }
        match entry.sample(contacts) {
            EntryEvent::Pending => std::thread::sleep(crate::buttons::POLL),
            EntryEvent::Progress(count) => show_pin_count(hal, prompt, count)?,
            EntryEvent::Restart => return Ok(None), // Mode cancels this screen.
            EntryEvent::Complete(pin) => {
                show_pin_count(hal, prompt, panel_input::PIN_LENGTH)?;
                return Ok(Some(pin));
            }
        }
    }
}

pub(super) fn run(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay + recovery_pin::PinContacts),
    dir: &Path,
    expected: [u8; 32],
) -> Result<(), Error> {
    let lock = File::open("/dev/i2c-1")?;
    lock.try_lock()?;
    let mut se = Se051::open("/dev/i2c-1")?;
    let (phase, identity) = inspect(&mut se, dir)?;
    if identity != Some(expected) {
        return Err("practice key changed".into());
    }
    verify_export(dir, &expected)?;
    if phase == Phase::Protected {
        if let Some(pin) = enter(hal, "Check signing PIN. #6 cancel")? {
            prove(&mut se, &pin, &expected)?;
            hal.show_message("Signing PIN verified. #5 OK")?;
            hal.wait_event()?;
        }
        return Ok(());
    }
    let backup = read_backup(dir)?;
    if backup.public_key != expected {
        return Err("backup differs from funded key".into());
    }
    if !confirm(
        hal,
        &format!(
            "Set up PIN for {}? Insert original recovery FIDO.",
            fingerprint(&expected)
        ),
    )? {
        return Ok(());
    }
    let seed = {
        let mut token = fido::open_one()?;
        token.check_file_backup_support()?;
        if token.pin_retries()? <= 1 {
            return Err("one or fewer FIDO PIN tries left".into());
        }
        let Some(pin) = recovery_pin::enter(hal)? else {
            return Ok(());
        };
        hal.show_message("Touch FIDO to unlock backup.")?;
        decrypt(&backup, &mut token, pin.pin())?
    };
    if SigningKey::from_bytes(&seed).verifying_key().to_bytes() != expected {
        return Err("recovery key mismatch".into());
    }
    let auth = exists(&mut se, AUTH)?;
    let prompt = if auth {
        "Enter signing PIN. #6 cancel"
    } else {
        "Choose signing PIN. Release each move. #6 cancel"
    };
    let Some(pin) = enter(hal, prompt)? else {
        return Ok(());
    };
    if !auth {
        let Some(repeat) = enter(hal, "Repeat signing PIN. #6 cancel")? else {
            return Ok(());
        };
        if *pin != *repeat {
            return Err("sequences differ; PIN was not enrolled".into());
        }
        if phase == Phase::Legacy {
            save_new(&dir.join(RECORD), &expected)?;
        }
        let rules = [policy(Flags::ALLOW_DELETE)];
        let cmd = crate::se051::enroll::EnrollSequence::new(AUTH, &pin, PolicySet(&rules), TRIES)
            .ok_or("invalid PIN")?;
        se.chip()
            .run_command(&cmd, &mut [0; 128])
            .map_err(chip_error)?;
    }
    if !exists(&mut se, PROTECTED)? {
        // Verify the credential before creating the dependent object on a resumed enrollment.
        with_pin(&mut se, &pin, |_, _| Ok(()))?;
        let rules = [policy(Flags::ALLOW_SIGN | Flags::ALLOW_DELETE)];
        se.chip()
            .run_command(
                &WriteEcKey {
                    transient: false,
                    is_auth: false,
                    key_type: Some(P1KeyType::KeyPair),
                    policy: Some(PolicySet(&rules)),
                    max_attempts: None,
                    object_id: PROTECTED,
                    curve: Some(EcCurve::IdEccEd25519),
                    private_key: Some(seed.as_slice()),
                    public_key: Some(&expected),
                },
                &mut [0; 256],
            )
            .map_err(chip_error)?;
    }
    drop(seed);
    prove(&mut se, &pin, &expected)?;
    drop(pin);
    if !confirm(
        hal,
        &format!(
            "PIN copy verified {}. Erase old unprotected copy?",
            fingerprint(&expected)
        ),
    )? {
        return Ok(());
    }
    if read_backup(dir)? != backup || inspect(&mut se, dir)? != (Phase::Pending, Some(expected)) {
        return Err("key or backup changed; keeping old copy".into());
    }
    erase_owned(&mut se, &expected)?;
    hal.show_message("Old copy removed. Signing PIN active. #5 OK")?;
    hal.wait_event()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interruption_never_reopens_legacy_restore() {
        let key = Some([7; 32]);
        assert_eq!(classify(key, None, false, false).unwrap(), Phase::Legacy);
        for (auth, protected) in [(false, false), (true, false), (true, true)] {
            assert_eq!(classify(key, key, auth, protected).unwrap(), Phase::Pending);
        }
        assert_eq!(classify(None, key, true, true).unwrap(), Phase::Protected);
        for (auth, protected) in [(true, false), (false, true), (true, true)] {
            assert!(classify(None, None, auth, protected).is_err());
        }
        assert!(classify(Some([8; 32]), key, true, true).is_err());
        assert!(classify(None, key, false, false).is_err());
    }

    #[test]
    fn all_contacts_match_the_existing_joystick_demo() {
        for (i, expected) in [64, 32, 1, 4, 2, 8].into_iter().enumerate() {
            assert_eq!(direction_mask(1 << i), expected);
        }
    }
}

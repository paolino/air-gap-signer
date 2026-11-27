//! Disposable lifecycle on a single dedicated SE051 object. The archive
//! survives process restarts. Its recovery secret stays on the FIDO token.
use crate::{fido, recovery_pin, se051::Se051};
use ed25519_dalek::SigningKey;
use se05x::se05x::ObjectId;
use sha2::{Digest, Sha256};
use signer_core::backup::{Backup, Seed, Token};
use signer_core::review::{ButtonEvent, View};
use signer_hal::{Buttons, Display, HalError};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

mod pin_enrollment;
pub mod preprod;
mod token_copy;

const TRIAL_KEY: ObjectId = ObjectId([0x7D, 0xA0, 0x32, 0x01]);
const ARCHIVE: &str = "practice-backup.cbor";
const PUBLIC: &str = "practice-public.key";
type Error = Box<dyn std::error::Error>;

struct Recovery<'a> {
    device: &'a mut fido::Device<File>,
    id: Vec<u8>,
    pin: &'a str,
}

impl Token for Recovery<'_> {
    fn credential_id(&self) -> Vec<u8> {
        self.id.clone()
    }
    fn secret(&mut self, salt: &[u8; 32]) -> Result<[u8; 32], String> {
        self.device
            .secret(&self.id, salt, self.pin)
            .map_err(|e| e.to_string())
    }
}

fn save_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let dir = path.parent().ok_or("missing trial directory")?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(path)?;
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn read_backup(dir: &Path) -> Result<Backup, Error> {
    let mut bytes = Vec::new();
    File::open(dir.join(ARCHIVE))?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("trial archive too large".into());
    }
    let backup = Backup::from_cbor(&bytes)?;
    if backup.tokens() != 1 {
        return Err("expected one enrolled trial token".into());
    }
    Ok(backup)
}

fn fingerprint(key: &[u8; 32]) -> String {
    hex::encode(&Sha256::digest(key)[..6])
}

pub fn confirm(hal: &mut (impl Buttons + Display), message: &str) -> Result<bool, HalError> {
    let (cols, height) = hal.text_grid();
    if cols < signer_core::review::MIN_COLS || height < signer_core::review::MIN_ROWS {
        return Err(HalError::Display(
            "Screen too small for confirmation".into(),
        ));
    }
    let lines: Vec<_> = message
        .lines()
        .flat_map(|line| crate::layout::wrap_message(line, cols))
        .collect();
    // Reserve the heading and one row per button.
    let mut pages = lines.chunks(height - 3).peekable();
    while let Some(page) = pages.next() {
        let last = pages.peek().is_none();
        hal.show_review(&confirmation_page(page, height, last))?;
        loop {
            match hal.wait_event()? {
                ButtonEvent::Confirm if last => return Ok(true),
                ButtonEvent::Confirm => break,
                ButtonEvent::Mode | ButtonEvent::Back | ButtonEvent::Reject => return Ok(false),
                _ => {}
            }
        }
    }
    Ok(false)
}

fn confirmation_page(page: &[String], height: usize, last: bool) -> View {
    let mut rows = page.to_vec();
    rows.resize(height - 3, String::new());
    let action = if last { "#5 Yes" } else { "#5 Next" };
    rows.push(action.into());
    rows.push("#6 Exit".into());
    View::Detail {
        header: "Confirm".into(),
        rows,
    }
}

fn show_public(hal: &mut (impl Buttons + Display), key: &[u8; 32]) -> Result<(), HalError> {
    let body = hal.text_grid().1.saturating_sub(1).max(1);
    let lines: Vec<_> = key.as_chunks::<8>().0.iter().map(hex::encode).collect();
    let pages = lines.len().div_ceil(body);
    let mut page = 0;
    loop {
        hal.show_review(&View::Detail {
            header: format!("Public {}/{}  #5 OK", page + 1, pages),
            rows: lines.iter().skip(body * page).take(body).cloned().collect(),
        })?;
        match hal.wait_event()? {
            ButtonEvent::Down | ButtonEvent::DownHeld => page = (page + 1).min(pages - 1),
            ButtonEvent::Up | ButtonEvent::UpHeld => page = page.saturating_sub(1),
            ButtonEvent::Confirm | ButtonEvent::Back | ButtonEvent::Mode => return Ok(()),
            _ => {}
        }
    }
}

fn decrypt(backup: &Backup, device: &mut fido::Device<File>, pin: &str) -> Result<Seed, Error> {
    let id = backup
        .credential_ids()
        .next()
        .ok_or("missing recovery credential")?
        .to_vec();
    Ok(backup.restore(&mut Recovery { device, id, pin })?)
}

fn verify_export(dir: &Path, public: &[u8; 32]) -> Result<(), Error> {
    if fs::read(dir.join(PUBLIC))?.as_slice() != public {
        return Err("saved public key differs; stopping".into());
    }
    Ok(())
}

fn erase_owned(se: &mut Se051, public: &[u8; 32]) -> Result<(), Error> {
    if se.read_public_object(TRIAL_KEY)? != Some(*public) {
        return Err("disposable key changed; stopping".into());
    }
    se.delete_object(TRIAL_KEY)?;
    if se.read_public_object(TRIAL_KEY)?.is_some() {
        return Err("test key still present".into());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    EnrollPin,
    Create,
    Restore,
    Verify,
    Erase,
    FileToToken,
    CopyToken,
    RecoverFile,
}

fn actions(current: Option<[u8; 32]>, saved: bool) -> &'static [(&'static str, Option<Action>)] {
    if current.is_some() {
        &[
            ("View public key", None),
            ("Enroll signing PIN", Some(Action::EnrollPin)),
            ("Verify backup", Some(Action::Verify)),
            ("Erase key", Some(Action::Erase)),
            ("File to FIDO", Some(Action::FileToToken)),
            ("Copy FIDO to FIDO", Some(Action::CopyToken)),
        ]
    } else if saved {
        &[
            ("Restore backup", Some(Action::Restore)),
            ("File to FIDO", Some(Action::FileToToken)),
            ("Copy FIDO to FIDO", Some(Action::CopyToken)),
        ]
    } else {
        &[
            ("Create + back up", Some(Action::Create)),
            ("Copy FIDO to FIDO", Some(Action::CopyToken)),
            ("Recover FIDO file", Some(Action::RecoverFile)),
        ]
    }
}

/// Return the public key currently present in the dedicated practice object.
pub fn current_public() -> Result<Option<[u8; 32]>, Error> {
    let bus = File::open("/dev/i2c-1")?;
    bus.try_lock()?;
    Ok(Se051::open("/dev/i2c-1")?.read_public_object(TRIAL_KEY)?)
}

/// Inspect the disposable key without entering a PIN. The owner can then
/// open the token-backed workflow, which rechecks the chip and saved archive.
pub fn menu(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay + recovery_pin::PinContacts),
    dir: &Path,
) -> Result<(), Error> {
    loop {
        use pin_enrollment::Phase;
        let (phase, current) = {
            let bus = File::open("/dev/i2c-1")?;
            bus.try_lock()?;
            pin_enrollment::inspect(&mut Se051::open("/dev/i2c-1")?, dir)?
        };
        let choices: &[(&str, Option<Action>)] = match phase {
            Phase::Legacy => actions(current, dir.join(ARCHIVE).try_exists()?),
            Phase::Pending => &[
                ("Finish PIN setup", Some(Action::EnrollPin)),
                ("View public key", None),
                ("File to FIDO", Some(Action::FileToToken)),
                ("Copy FIDO to FIDO", Some(Action::CopyToken)),
            ],
            Phase::Protected => &[
                ("Check signing PIN", Some(Action::EnrollPin)),
                ("View public key", None),
                ("File to FIDO", Some(Action::FileToToken)),
                ("Copy FIDO to FIDO", Some(Action::CopyToken)),
            ],
        };
        let mut selected = 0usize;
        let header = current
            .map(|key| format!("Practice {}", fingerprint(&key)))
            .unwrap_or_else(|| "Practice key: empty".into());
        loop {
            let visible = hal.text_grid().1.saturating_sub(1).max(1);
            hal.show_review(&View::Overview {
                header: header.clone(),
                rows: choices
                    .iter()
                    .map(|(label, _)| *label)
                    .chain(["Back"])
                    .enumerate()
                    .skip((selected + 1).saturating_sub(visible))
                    .take(visible)
                    .map(|(i, label)| signer_core::review::Row {
                        text: label.into(),
                        selected: i == selected,
                        pending: false,
                        waived: false,
                    })
                    .collect(),
            })?;
            match hal.wait_event()? {
                ButtonEvent::Up => selected = (selected + choices.len()) % (choices.len() + 1),
                ButtonEvent::Down => selected = (selected + 1) % (choices.len() + 1),
                ButtonEvent::UpHeld => selected = selected.saturating_sub(1),
                ButtonEvent::DownHeld => selected = (selected + 1).min(choices.len()),
                ButtonEvent::Back | ButtonEvent::Mode => return Ok(()),
                ButtonEvent::Open | ButtonEvent::Confirm => {
                    let Some((_, action)) = choices.get(selected) else {
                        return Ok(());
                    };
                    if *action == Some(Action::EnrollPin) {
                        pin_enrollment::run(
                            hal,
                            dir,
                            current.ok_or("missing practice public key")?,
                        )?;
                    } else if let Some(
                        action @ (Action::FileToToken | Action::CopyToken | Action::RecoverFile),
                    ) = action
                    {
                        token_copy::run(hal, dir, *action, current)?;
                    } else if let Some(action) = action {
                        lifecycle(hal, dir, Some((*action, current)))?;
                    } else if let Some(key) = current {
                        show_public(hal, &key)?;
                    }
                    break;
                }
                _ => {}
            }
        }
    }
}

/// Run again with the same directory and token after the first erase.
pub fn run(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay + recovery_pin::PinContacts),
    dir: &Path,
) -> Result<(), Error> {
    lifecycle(hal, dir, None)
}

fn lifecycle(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay + recovery_pin::PinContacts),
    dir: &Path,
    request: Option<(Action, Option<[u8; 32]>)>,
) -> Result<(), Error> {
    // Serialise all instances even when started with different archive paths.
    let bus = File::open("/dev/i2c-1")?;
    bus.try_lock()?;
    let mut se = Se051::open("/dev/i2c-1")?;
    let (phase, current) = pin_enrollment::inspect(&mut se, dir)?;
    if phase != pin_enrollment::Phase::Legacy {
        return Err("use the PIN-protected practice menu".into());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let saved = dir.join(ARCHIVE).try_exists()?;
    let public_record = dir.join(PUBLIC);
    if !saved && (current.is_some() || public_record.try_exists()?) {
        return Err("scratch object or public record already exists; stopping".into());
    }
    let existing = if saved { Some(read_backup(dir)?) } else { None };
    if let (Some(key), Some(backup)) = (current, &existing) {
        if key != backup.public_key {
            return Err("scratch key belongs to another trial".into());
        }
    }
    let restoring = saved && current.is_none();
    if let Some((action, selected)) = request {
        if selected != current
            || !actions(current, saved)
                .iter()
                .any(|(_, a)| *a == Some(action))
        {
            return Err("practice key or backup changed; reselect the action".into());
        }
    }
    let action = request.map(|(action, _)| action);
    let prompt = match (action, restoring, saved) {
        (Some(Action::Erase), _, _) => "Check backup before erasing practice key?",
        (_, true, _) => "Restore disposable key from saved backup?",
        (_, false, true) => "Verify saved backup with token?",
        _ => "Create disposable key and encrypted backup?",
    };
    if !confirm(hal, prompt)? {
        return Ok(());
    }
    let mut device = fido::open()?;
    device.check_file_backup_support()?;
    if device.pin_retries()? <= 1 {
        return Err("token has at most one PIN try left; stopping".into());
    }
    let Some(pin) = recovery_pin::enter(hal)? else {
        return Ok(());
    };
    if existing.is_none() {
        hal.show_message(
            "Touch recovery token to enroll. Touch again when it flashes to encrypt.",
        )?;
        let id = device.enrol_file(pin.pin())?;
        let seed = se.fresh_seed()?;
        let backup = Backup::create(
            &seed,
            &mut [&mut Recovery {
                device: &mut device,
                id,
                pin: pin.pin(),
            }],
        )?;
        save_new(&dir.join(ARCHIVE), &backup.to_cbor())?;
    }
    let backup = read_backup(dir)?;
    hal.show_message("Touch recovery token to check the saved backup.")?;
    let seed = decrypt(&backup, &mut device, pin.pin())?;
    drop(pin);
    let expected = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    if expected != backup.public_key {
        return Err("recovery public key mismatch".into());
    }
    if !public_record.try_exists()? {
        save_new(&public_record, &expected)?;
    }
    verify_export(dir, &expected)?;
    if current.is_none() {
        se.write_object(TRIAL_KEY, &seed)?;
    }
    drop(seed);
    if se.read_public_object(TRIAL_KEY)? != Some(expected) {
        return Err("chip and backup public keys differ".into());
    }
    // Prove possession of the corresponding private key, even when the chip
    // stores an independently supplied public key. This uses a random test message.
    let challenge = rand::random::<[u8; 32]>();
    let signature = se.sign_object(TRIAL_KEY, &challenge)?;
    crate::se051::verify(&expected, &challenge, &signature)?;
    let label = fingerprint(&expected);
    if let Some(action) = action {
        if action != Action::Erase {
            let verb = match action {
                Action::Restore => "Restored",
                Action::Create => "Backed up",
                _ => "Verified",
            };
            hal.show_message(&format!("{verb} {label}.\n#5 OK"))?;
            hal.wait_event()?;
            return Ok(());
        }
    } else {
        show_public(hal, &expected)?;
    }
    let prompt = if restoring {
        format!("Restored {label}. Test signature verified. Erase this test key to finish?")
    } else {
        format!("Backup verified. Erase test key {label} from this signer?")
    };
    if !confirm(hal, &prompt)? {
        return Ok(());
    }
    verify_export(dir, &expected)?;
    if read_backup(dir)? != backup {
        return Err("saved backup changed; keeping test key".into());
    }
    erase_owned(&mut se, &expected)?;
    let message = if action == Some(Action::Erase) {
        "Practice key erased. Backup kept.\n#5 OK"
    } else if restoring {
        "Trial complete. Restored key matched. Test slot empty."
    } else {
        "Test slot empty. Restart the trial to restore the encrypted backup."
    };
    hal.show_message(message)?;
    if action == Some(Action::Erase) {
        hal.wait_event()?;
    }
    println!("{message}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_actions_have_a_separate_bottom_row_at_each_grid_size() {
        for height in [4, 11] {
            let body = vec!["Erase key?".to_string(); height - 3];
            for (last, action) in [(false, "#5 Next"), (true, "#5 Yes")] {
                let View::Detail { rows, .. } = confirmation_page(&body, height, last) else {
                    panic!("expected a question with a footer");
                };
                assert_eq!(&rows[..body.len()], &body);
                assert_eq!(rows[height - 3], action);
                assert_eq!(rows[height - 2], "#6 Exit");
            }
        }
    }

    #[test]
    fn practice_actions_follow_key_and_backup_state() {
        let labels = |key, saved| {
            actions(key, saved)
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(None, false),
            ["Create + back up", "Copy FIDO to FIDO", "Recover FIDO file"]
        );
        assert_eq!(
            labels(None, true),
            ["Restore backup", "File to FIDO", "Copy FIDO to FIDO"]
        );
        assert_eq!(
            labels(Some([0; 32]), true),
            [
                "View public key",
                "Enroll signing PIN",
                "Verify backup",
                "Erase key",
                "File to FIDO",
                "Copy FIDO to FIDO"
            ]
        );
    }
    struct TestToken(u8);
    impl Token for TestToken {
        fn credential_id(&self) -> Vec<u8> {
            vec![self.0]
        }
        fn secret(&mut self, salt: &[u8; 32]) -> Result<[u8; 32], String> {
            let mut h = Sha256::new();
            h.update([self.0]);
            h.update(salt);
            Ok(h.finalize().into())
        }
    }

    #[test]
    fn fresh_process_inputs_recover_only_with_the_enrolled_token_and_the_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ARCHIVE);
        let seed = Seed::new([0x5a; 32]);
        let backup = Backup::create(&seed, &mut [&mut TestToken(7)]).unwrap();
        save_new(&path, &backup.to_cbor()).unwrap();
        save_new(&dir.path().join(PUBLIC), &backup.public_key).unwrap();
        drop(backup);
        drop(seed);
        assert!(save_new(&path, b"replacement").is_err());
        let saved = read_backup(dir.path()).unwrap();
        assert_eq!(*saved.restore(&mut TestToken(7)).unwrap(), [0x5a; 32]);
        assert!(saved.restore(&mut TestToken(8)).is_err());
        verify_export(dir.path(), &saved.public_key).unwrap();
        assert!(verify_export(dir.path(), &[0; 32]).is_err());
        fs::write(&path, b"truncated").unwrap();
        assert!(read_backup(dir.path()).is_err());
    }
}

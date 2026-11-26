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
use std::path::Path;

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

fn confirm(hal: &mut (impl Buttons + Display), message: &str) -> Result<bool, HalError> {
    hal.show_message(&format!("{message}\n#5 yes  #6 exit"))?;
    loop {
        match hal.wait_event()? {
            ButtonEvent::Confirm => return Ok(true),
            ButtonEvent::Mode | ButtonEvent::Back | ButtonEvent::Reject => return Ok(false),
            _ => {}
        }
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

/// Run again with the same directory and token after the first erase.
pub fn run(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay),
    dir: &Path,
) -> Result<(), Error> {
    // Serialise all instances even when started with different archive paths.
    let bus = File::open("/dev/i2c-1")?;
    bus.try_lock()?;
    let mut se = Se051::open("/dev/i2c-1")?;
    let current = se.read_public_object(TRIAL_KEY)?;
    fs::create_dir_all(dir)?;
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
    let prompt = match (restoring, saved) {
        (true, _) => "Restore disposable key from saved backup?",
        (false, true) => "Resume disposable key trial?",
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
    show_public(hal, &expected)?;
    let label = fingerprint(&expected);
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
    let message = if restoring {
        "Trial complete. Restored key matched. Test slot empty."
    } else {
        "Test slot empty. Restart the trial to restore the encrypted backup."
    };
    hal.show_message(message)?;
    println!("{message}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

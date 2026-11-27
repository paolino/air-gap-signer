//! Standalone, single-token practice archives. The two USB sessions never overlap.
use super::*;

fn decoded(stored: &fido::StoredBackup) -> Result<Backup, Error> {
    let backup = Backup::from_cbor(&stored.data)?;
    if backup.tokens() != 1 || backup.credential_ids().next() != Some(stored.id.as_slice()) {
        return Err("backup belongs to a different credential".into());
    }
    Ok(backup)
}

fn destination_archive(
    stored: &fido::StoredBackup,
    public: &[u8; 32],
    excluded: Option<&[u8]>,
) -> Result<Backup, Error> {
    if excluded == Some(stored.id.as_slice()) {
        return Err("source FIDO reinserted; use another".into());
    }
    let archive = decoded(stored)?;
    if archive.public_key != *public {
        return Err("destination holds another backup".into());
    }
    Ok(archive)
}

fn connect(resident: bool) -> Result<fido::Device<File>, Error> {
    let mut dev = fido::open_one()?;
    if resident {
        dev.check_resident_backup_support()?;
    } else {
        dev.check_file_backup_support()?;
    }
    if dev.pin_retries()? <= 1 {
        return Err("one or fewer PIN tries left".into());
    }
    Ok(dev)
}

pub(super) fn run(
    hal: &mut (impl Buttons + Display + recovery_pin::PinDisplay + recovery_pin::PinContacts),
    dir: &Path,
    action: Action,
    current: Option<[u8; 32]>,
) -> Result<(), Error> {
    let from_file = action == Action::FileToToken;
    let recover_file = action == Action::RecoverFile;
    let bus = File::open("/dev/i2c-1")?;
    bus.try_lock()?;
    if !confirm(
        hal,
        if from_file {
            "Insert the FIDO that unlocks the practice file."
        } else {
            "Insert source FIDO A. Read its practice backup?"
        },
    )? {
        return Ok(());
    }
    let (backup, seed) = {
        let mut source = connect(!from_file)?;
        let Some(pin) = recovery_pin::enter(hal)? else {
            return Ok(());
        };
        hal.show_message("Read source. Touch token when flashing.")?;
        let backup = if from_file {
            read_backup(dir)?
        } else {
            decoded(
                &source
                    .resident_backup(pin.pin())?
                    .ok_or("no backup on source FIDO")?,
            )?
        };
        if current.is_some_and(|key| key != backup.public_key) {
            return Err("source differs from panel practice key".into());
        }
        if dir.join(PUBLIC).try_exists()? {
            verify_export(dir, &backup.public_key)?;
        }
        if dir.join(ARCHIVE).try_exists()?
            && !from_file
            && read_backup(dir)?.public_key != backup.public_key
        {
            return Err("source differs from panel archive".into());
        }
        let seed = decrypt(&backup, &mut source, pin.pin())?;
        (backup, seed)
    };
    let label = fingerprint(&backup.public_key);
    if recover_file {
        drop(seed);
        if !confirm(hal, &format!("Recover file {label} here?"))? {
            return Ok(());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        save_new(&dir.join(ARCHIVE), &backup.to_cbor())?;
        hal.show_message("File recovered. Choose Restore backup with this FIDO.\n#5 OK")?;
        hal.wait_event()?;
        return Ok(());
    }
    let prompt = if from_file {
        "Insert FIDO to hold backup. Same token OK."
    } else {
        "Remove A. Insert new FIDO B only."
    };
    if !confirm(hal, &format!("{label}\n{prompt}"))? {
        return Ok(());
    }
    let mut destination = connect(true)?;
    let Some(pin) = recovery_pin::enter(hal)? else {
        return Ok(());
    };
    let pin = pin.pin();
    hal.show_message("Check destination. Touch when flashing.")?;
    let excluded = if from_file {
        None
    } else {
        backup.credential_ids().next()
    };
    if let Some(stored) = destination.resident_backup(pin)? {
        destination_archive(&stored, &backup.public_key, excluded)?;
    } else {
        if !confirm(hal, &format!("Save {label} on this FIDO?"))? {
            return Ok(());
        }
        hal.show_message("Writing backup. Touch FIDO when flashing.")?;
        let id = destination.enrol_copy(pin, excluded)?;
        let copy = Backup::create(
            &seed,
            &mut [&mut Recovery {
                device: &mut destination,
                id: id.clone(),
                pin,
            }],
        )?;
        destination.store_backup(&id, &copy.to_cbor(), pin)?;
    }
    drop(seed);
    hal.show_message("Checking saved copy. Touch when flashing.")?;
    let stored = destination
        .resident_backup(pin)?
        .ok_or("saved copy missing")?;
    let saved = destination_archive(&stored, &backup.public_key, excluded)?;
    drop(decrypt(&saved, &mut destination, pin)?);
    hal.show_message(&format!("FIDO backup verified\n{label}\n#5 OK"))?;
    hal.wait_event()?;
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
            Ok(Sha256::digest([salt.as_slice(), &[self.0]].concat()).into())
        }
    }

    #[test]
    fn a_fresh_panel_recovers_b_without_a_or_the_old_file() {
        let (mut a, mut b) = (TestToken(1), TestToken(2));
        let original = Backup::create(&Seed::new([42; 32]), &mut [&mut a]).unwrap();
        let source = fido::StoredBackup {
            id: a.credential_id(),
            data: original.to_cbor(),
        };
        let seed = decoded(&source).unwrap().restore(&mut a).unwrap();
        let copy = Backup::create(&seed, &mut [&mut b]).unwrap();
        drop(seed);
        let array = crate::large_blob::write(
            &crate::large_blob::serialize(&[]),
            &[5; 32],
            &copy.to_cbor(),
        )
        .unwrap();
        let stored = fido::StoredBackup {
            id: b.credential_id(),
            data: crate::large_blob::read(&array, &[5; 32]).unwrap(),
        };
        let excluded = Some(source.id.as_slice());
        assert!(destination_archive(&source, &original.public_key, excluded).is_err());
        assert!(destination_archive(&stored, &[0; 32], excluded).is_err());
        let dir = tempfile::tempdir().unwrap();
        save_new(
            &dir.path().join(ARCHIVE),
            &destination_archive(&stored, &original.public_key, excluded)
                .unwrap()
                .to_cbor(),
        )
        .unwrap();
        let saved = read_backup(dir.path()).unwrap();
        assert_eq!(*saved.restore(&mut b).unwrap(), [42; 32]);
        assert!(saved.restore(&mut a).is_err());
        assert_eq!(
            *decoded(&source).unwrap().restore(&mut a).unwrap(),
            [42; 32]
        );
        assert!(decoded(&fido::StoredBackup {
            id: a.credential_id(),
            data: stored.data
        })
        .is_err());
    }
}

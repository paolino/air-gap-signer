//! Experiment: does the SE051 take an Ed25519 key as an RFC 8032 seed?
//! Loads the RFC 8032 test 1 secret (both byte orders), reads back the
//! public key the chip derives and signs the empty message.
//!
//! Result on the rig (applet 7.2): with the secret as written the chip
//! returns the RFC signature with R and S each byte-reversed, so it takes
//! a standard seed (hashed inside) — not a Cardano extended key. Reading
//! back a private-only object is refused (ConditionsOfUseNotSatisfied).

#[cfg(target_os = "linux")]
fn main() {
    use se05x::se05x::commands::{DeleteSecureObject, EddsaSign, ReadObject, WriteEcKey};
    use se05x::se05x::{EcCurve, ObjectId, P1KeyType};
    const SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    const SIG: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";
    let mut se = signer_pi::se051::Se051::open("/dev/i2c-1").expect("open");
    let chip = se.chip();
    let id = ObjectId([0x7D, 0xA0, 0x00, 0x01]);
    for (name, reversed) in [("as written", false), ("reversed", true)] {
        let mut secret = hex::decode(SECRET).unwrap();
        if reversed {
            secret.reverse();
        }
        let buf = &mut [0u8; 512];
        let _ = chip.run_command(&DeleteSecureObject { object_id: id }, buf);
        let w = WriteEcKey {
            transient: false,
            is_auth: false,
            key_type: Some(P1KeyType::Private),
            policy: None,
            max_attempts: None,
            object_id: id,
            curve: Some(EcCurve::IdEccEd25519),
            private_key: Some(&secret),
            public_key: None,
        };
        if let Err(e) = chip.run_command(&w, buf) {
            println!("{name}: write failed: {e:?}");
            continue;
        }
        let read = ReadObject {
            object_id: id,
            offset: None,
            length: None,
            rsa_key_component: None,
        };
        let public = chip.run_command(&read, buf).map(|r| hex::encode(r.data));
        let buf = &mut [0u8; 512];
        let sig = chip
            .run_command(
                &EddsaSign {
                    key_id: id,
                    data: b"",
                },
                buf,
            )
            .map(|r| hex::encode(r.signature));
        let rev = |h: &str| {
            let mut b = hex::decode(h).unwrap();
            b.reverse();
            hex::encode(b)
        };
        println!("{name}: public {public:?}");
        println!("  RFC public {PUBLIC} reversed {}", rev(PUBLIC));
        println!("{name}: signature {sig:?}");
        println!(
            "  RFC signature matches: {}",
            sig.as_deref().ok() == Some(SIG)
        );
    }
    let buf = &mut [0u8; 64];
    let _ = chip.run_command(&DeleteSecureObject { object_id: id }, buf);
}

#[cfg(not(target_os = "linux"))]
fn main() {}

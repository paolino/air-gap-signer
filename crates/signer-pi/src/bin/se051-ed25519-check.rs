//! Known-answer check: the SE051 takes an Ed25519 key as an RFC 8032 seed.
//!
//! Loads the RFC 8032 test 1 secret into a scratch object, signs the
//! empty message and requires the RFC signature (the chip returns R and
//! S each byte-reversed). Exits non-zero on any error or mismatch. The
//! scratch object must not exist beforehand, and only an object this run
//! created is deleted.
//!
//! Result on the rig (applet 7.2): passes — the chip hashes a standard
//! seed, so Cardano extended (CIP-1852) keys cannot be loaded; reading a
//! private-only object is refused (ConditionsOfUseNotSatisfied).

#[cfg(target_os = "linux")]
fn main() {
    use se05x::se05x::commands::{CheckObjectExists, DeleteSecureObject, EddsaSign, WriteEcKey};
    use se05x::se05x::{EcCurve, ObjectId, P1KeyType};
    const SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const SIG: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";
    let fail = |msg: String| -> ! {
        eprintln!("FAIL {msg}");
        std::process::exit(1)
    };
    let mut se =
        signer_pi::se051::Se051::open("/dev/i2c-1").unwrap_or_else(|e| fail(e.to_string()));
    let chip = se.chip();
    let id = ObjectId([0x7D, 0xA0, 0x00, 0x01]);
    let buf = &mut [0u8; 512];
    let exists = chip
        .run_command(&CheckObjectExists { object_id: id }, buf)
        .unwrap_or_else(|e| fail(format!("check scratch object: {e:?}")))
        .result
        .is_success();
    if exists {
        fail(format!("scratch object {id:?} exists; not touching it"));
    }
    let secret = hex::decode(SECRET).unwrap();
    let write = WriteEcKey {
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
    if let Err(e) = chip.run_command(&write, buf) {
        fail(format!("load the RFC secret: {e:?}"));
    }
    let signed = chip
        .run_command(
            &EddsaSign {
                key_id: id,
                data: b"",
            },
            buf,
        )
        .map(|r| r.signature.to_vec());
    let deleted = chip.run_command(&DeleteSecureObject { object_id: id }, &mut [0u8; 64]);
    let signature = signed.unwrap_or_else(|e| fail(format!("sign: {e:?}")));
    if signature.len() != 64 {
        fail(format!("signature of {} bytes", signature.len()));
    }
    let mut rfc_order = signature.clone();
    rfc_order[..32].reverse();
    rfc_order[32..].reverse();
    if hex::encode(&rfc_order) != SIG {
        fail(format!(
            "signature {} is not the RFC one",
            hex::encode(&signature)
        ));
    }
    if let Err(e) = deleted {
        fail(format!("delete the scratch object: {e:?}"));
    }
    println!("PASS the chip signs the RFC 8032 test 1 vector (R and S byte-reversed)");
}

#[cfg(not(target_os = "linux"))]
fn main() {}

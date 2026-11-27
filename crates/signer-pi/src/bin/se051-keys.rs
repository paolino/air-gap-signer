//! The SE051 key slots from the command line, for acceptance
//! (`scripts/se051-acceptance.sh`):
//!
//! - `se051-keys create <slot>` puts a fresh key in an empty slot and
//!   prints its public key (hex);
//! - `se051-keys public <slot>` prints it;
//! - `se051-keys practice-public` reads the dedicated practice object;
//! - `se051-keys sign <slot> <message hex>` prints the signature (hex),
//!   already checked against the public key;
//! - `se051-keys delete <slot>` erases the key.

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = || -> Result<String, Box<dyn std::error::Error>> {
        if args.as_slice() == ["practice-public"] {
            return Ok(signer_pi::key_trial::current_public()?
                .map(hex::encode)
                .unwrap_or_else(|| "empty".into()));
        }
        let mut se = signer_pi::se051::Se051::open("/dev/i2c-1")?;
        let slot = |s: &str| s.parse::<u8>();
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["list"] => Ok(se
                .public_slots()?
                .into_iter()
                .enumerate()
                .map(|(slot, key)| {
                    format!(
                        "slot {slot} {}",
                        key.map(hex::encode).unwrap_or_else(|| "empty".into())
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            ["create", s] => {
                let seed = se.fresh_seed()?;
                Ok(hex::encode(se.put_key(slot(s)?, &seed)?))
            }
            ["public", s] => Ok(se
                .public_key(slot(s)?)?
                .map(hex::encode)
                .unwrap_or_else(|| "empty".into())),
            ["sign", s, msg] => Ok(hex::encode(se.sign(slot(s)?, &hex::decode(msg)?)?)),
            ["delete", s] => {
                se.delete_key(slot(s)?)?;
                Ok("deleted".into())
            }
            _ => Err(
                "usage: se051-keys practice-public | list | create|public|delete <slot> | sign <slot> <message hex>"
                    .into(),
            ),
        }
    };
    match run() {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}

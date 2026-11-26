//! Commands for the SE051 built byte by byte, for what the `se05x`
//! driver cannot express: secure-messaging wrappers, policies with a PCR
//! requirement, a two-byte retry limit on a UserID.

use iso7816::command::{DataSource, DataStream, Writer};
use se05x::se05x::{Se05XCommand, Se05XResponse};

/// A command as raw bytes.
pub struct Apdu(pub Vec<u8>);
/// A response's data, unparsed.
pub struct Reply<'a>(pub &'a [u8]);

impl DataSource for Apdu {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl<W: Writer> DataStream<W> for Apdu {
    fn to_writer(&self, writer: &mut W) -> Result<(), W::Error> {
        writer.write_all(&self.0)
    }
}
impl<W: Writer> Se05XCommand<W> for Apdu {
    type Response<'a> = Reply<'a>;
}
impl<'a> Se05XResponse<'a> for Reply<'a> {
    fn from_response(data: &'a [u8]) -> Result<Self, se05x::se05x::Error> {
        Ok(Reply(data))
    }
}

/// The bytes the driver would send for `command`.
pub fn bytes<C: for<'a> DataStream<&'a mut [u8]>>(command: &C) -> Vec<u8> {
    let mut buf = [0u8; 512];
    let mut writer: &mut [u8] = &mut buf[..];
    command.to_writer(&mut writer).expect("in-memory APDU");
    let written = 512 - writer.len();
    buf[..written].to_vec()
}

/// A TLV with BER length.
pub fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match value.len() {
        n @ 0..=0x7f => out.push(n as u8),
        n @ 0x80..=0xff => out.extend([0x81, n as u8]),
        n => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend_from_slice(value);
    out
}

/// A command from its header and data, short when it fits, else
/// extended.
pub fn command(header: [u8; 4], data: &[u8], le: Option<u16>) -> Vec<u8> {
    let mut out = header.to_vec();
    if data.len() <= 255 && le.is_none_or(|l| l <= 256) {
        if !data.is_empty() {
            out.push(data.len() as u8);
            out.extend_from_slice(data);
        }
        if let Some(le) = le {
            out.push(le as u8);
        }
    } else {
        out.push(0);
        if !data.is_empty() {
            out.extend_from_slice(&(data.len() as u16).to_be_bytes());
            out.extend_from_slice(data);
        }
        if let Some(le) = le {
            out.extend_from_slice(&le.to_be_bytes());
        }
    }
    out
}

/// One policy entry: credential `auth`, access `flags`, and optionally a
/// PCR object that must hold `value`.
pub fn policy(auth: [u8; 4], flags: u32, pcr: Option<([u8; 4], [u8; 32])>) -> Vec<u8> {
    const REQUIRE_PCR_VALUE: u32 = 0x0001_0000;
    let mut entry = auth.to_vec();
    match pcr {
        None => entry.extend(flags.to_be_bytes()),
        Some((id, value)) => {
            entry.extend((flags | REQUIRE_PCR_VALUE).to_be_bytes());
            entry.extend(id);
            entry.extend(value);
        }
    }
    let mut out = vec![entry.len() as u8];
    out.extend(entry);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlvs_use_ber_lengths() {
        assert_eq!(tlv(0x41, &[1, 2]), vec![0x41, 2, 1, 2]);
        assert_eq!(&tlv(0x41, &[0; 200])[..3], &[0x41, 0x81, 200]);
    }

    #[test]
    fn commands_are_short_when_they_fit() {
        assert_eq!(
            command([0x80, 1, 2, 3], &[9], None),
            vec![0x80, 1, 2, 3, 1, 9]
        );
        assert_eq!(
            command([0x80, 1, 2, 3], &[9], Some(0)),
            vec![0x80, 1, 2, 3, 1, 9, 0]
        );
        assert_eq!(
            &command([0x80, 1, 2, 3], &[0; 300], None)[4..7],
            &[0, 1, 44]
        );
    }

    #[test]
    fn policies_match_the_driver_encoding() {
        use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags as F, Policy, PolicySet};
        use se05x::se05x::ObjectId;
        let id = [0x7D, 0xA0, 0x30, 0x01];
        let ours = policy(id, F::ALLOW_SIGN.union(F::REQUIRE_SM).bits(), None);
        let set = [Policy {
            object_id: ObjectId(id),
            access_rule: ObjectAccessRule::from_flags(F::ALLOW_SIGN.union(F::REQUIRE_SM)),
        }];
        let mut buf = [0u8; 64];
        assert_eq!(ours, PolicySet(&set).to_bytes(&mut buf).unwrap());
        let with_pcr = policy(id, F::ALLOW_SIGN.bits(), Some(([1; 4], [2; 32])));
        assert_eq!(with_pcr[0] as usize, 4 + 4 + 4 + 32);
        assert_eq!(
            &with_pcr[5..9],
            &(F::ALLOW_SIGN.bits() | 0x0001_0000).to_be_bytes()
        );
    }
}

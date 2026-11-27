//! Version one of the preprod panel's bounded Bluetooth mailbox.
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const SERVICE: Uuid = Uuid::from_u128(0x8d36c836_c015_4da1_8d7a_253247100001);
pub const KEY: Uuid = Uuid::from_u128(0x8d36c836_c015_4da1_8d7a_253247100002);
pub const REQUEST: Uuid = Uuid::from_u128(0x8d36c836_c015_4da1_8d7a_253247100003);
pub const RESULT: Uuid = Uuid::from_u128(0x8d36c836_c015_4da1_8d7a_253247100004);
pub const CLOCK: Uuid = Uuid::from_u128(0x8d36c836_c015_4da1_8d7a_253247100005);
const MAX_PAYLOAD: usize = 1024;
const MAX_PLAN: usize = 4096;
const LIMIT: usize = 2 + MAX_PAYLOAD + MAX_PLAN;
pub type Id = [u8; 16];

pub fn encode(payload: &[u8], plan: &[u8]) -> Result<Vec<u8>, &'static str> {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD || plan.is_empty() || plan.len() > MAX_PLAN
    {
        return Err("preprod request exceeds panel limits or is empty");
    }
    let mut data = (payload.len() as u16).to_le_bytes().to_vec();
    data.extend_from_slice(payload);
    data.extend_from_slice(plan);
    Ok(data)
}

pub fn decode(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (header, content) = data.split_at_checked(2)?;
    let len = u16::from_le_bytes(header.try_into().ok()?) as usize;
    let (tx, plan) = content.split_at_checked(len)?;
    (len > 0 && len <= MAX_PAYLOAD && !plan.is_empty() && plan.len() <= MAX_PLAN)
        .then_some((tx, plan))
}

pub fn id(data: &[u8]) -> Id {
    Sha256::digest(data)[..16]
        .try_into()
        .expect("16 byte prefix")
}

// Every acknowledged write fits even the mandatory minimum ATT MTU of 23.
pub fn frames(data: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = vec![[&[1][..], &id(data)].concat()];
    for (index, chunk) in data.chunks(17).enumerate() {
        let offset = ((index * 17) as u16).to_le_bytes();
        frames.push([&[2][..], &offset, chunk].concat());
    }
    frames.push(vec![3]);
    frames
}

#[derive(Default)]
pub struct Mailbox {
    upload: Option<(Id, Vec<u8>)>,
    // State 1 reviewing, 2 declined, 3 signed, 4 failed. Keep the most recent
    // outcome until the next distinct request is admitted.
    result: Vec<u8>,
}

impl Mailbox {
    pub fn result(&self) -> &[u8] {
        &self.result
    }

    pub fn write(&mut self, frame: &[u8]) -> Result<Option<Vec<u8>>, &'static str> {
        match frame {
            [1, tag @ ..] if tag.len() == 16 => {
                if self.result.first() == Some(&1) {
                    return Err("panel busy");
                }
                self.upload = Some((tag.try_into().unwrap(), Vec::new()));
            }
            [2, low, high, chunk @ ..] if !chunk.is_empty() && chunk.len() <= 17 => {
                let (_, data) = self.upload.as_mut().ok_or("start upload first")?;
                if u16::from_le_bytes([*low, *high]) as usize != data.len()
                    || data.len() + chunk.len() > LIMIT
                {
                    return Err("invalid upload offset or size");
                }
                data.extend_from_slice(chunk);
            }
            [3] => {
                let (tag, data) = self.upload.take().ok_or("missing upload")?;
                if id(&data) != tag || decode(&data).is_none() {
                    return Err("invalid request");
                }
                if self.result.get(1..17) != Some(&tag) || self.result.first() == Some(&4) {
                    self.result = [&[1][..], &tag].concat();
                    return Ok(Some(data));
                }
            }
            _ => return Err("invalid frame"),
        }
        Ok(None)
    }

    pub fn finish(&mut self, tag: Id, outcome: Result<Option<[u8; 64]>, ()>) {
        if self.result.first() != Some(&1) || self.result.get(1..17) != Some(&tag) {
            return;
        }
        self.result[0] = match outcome {
            Ok(Some(sig)) => {
                self.result.extend_from_slice(&sig);
                3
            }
            Ok(None) => 2,
            Err(()) => 4,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_and_duplicate_cannot_review_twice() {
        let data = encode(&[4; 1024], &[7; 4096]).unwrap();
        let packets = frames(&data);
        let mut m = Mailbox::default();
        assert!(m.write(&packets[1]).is_err());
        for p in &packets[..4] {
            assert!(m.write(p).unwrap().is_none());
        }
        assert!(m.write(&packets[2]).is_err());
        for p in &packets[..packets.len() - 1] {
            assert!(m.write(p).unwrap().is_none());
        }
        assert_eq!(m.write(&[3]).unwrap(), Some(data.clone()));
        assert!(m.write(&packets[0]).is_err());
        m.finish(id(&data), Ok(Some([42; 64])));
        for p in &packets {
            assert!(m.write(p).unwrap().is_none());
        }
        assert_eq!(&m.result()[17..], &[42; 64]);
    }
    #[test]
    fn an_explicit_resend_retries_a_failed_review_but_preserves_a_decline() {
        let data = encode(&[1], &[2]).unwrap();
        let mut mailbox = Mailbox::default();
        let send = |mailbox: &mut Mailbox| {
            frames(&data)
                .iter()
                .find_map(|frame| mailbox.write(frame).unwrap())
        };
        assert_eq!(send(&mut mailbox), Some(data.clone()));
        mailbox.finish(id(&data), Err(()));
        assert_eq!(send(&mut mailbox), Some(data.clone()));
        mailbox.finish(id(&data), Ok(None));
        assert_eq!(send(&mut mailbox), None);
        assert_eq!(mailbox.result()[0], 2);
    }
    #[test]
    fn tampered_commit_never_reaches_panel() {
        let mut frames = frames(&encode(&[1], &[2]).unwrap());
        frames[1][5] ^= 1;
        let mut m = Mailbox::default();
        m.write(&frames[0]).unwrap();
        m.write(&frames[1]).unwrap();
        assert!(m.write(&frames[2]).is_err());
        assert!(m.result().is_empty());
    }
}

//! AN12543 WriteUserID: TAG_MAX_ATTEMPTS occupies two bytes on this applet.
//! se05x 0.4.2's WriteUserId encodes it as one byte.
use iso7816::command::{class::NO_SM_CLA, CommandBuilder, DataSource, DataStream, Writer};
use iso7816::tlv::Tlv;
use se05x::se05x::{
    policies::PolicySet, Be, ObjectId, Se05XCommand, INS_AUTH_OBJECT, INS_WRITE, P1_USERID,
    P2_DEFAULT, TAG_1, TAG_2, TAG_MAX_ATTEMPTS, TAG_POLICY,
};
use zeroize::Zeroizing;

pub struct EnrollSequence {
    data: Zeroizing<[u8; 96]>,
    used: usize,
}

impl EnrollSequence {
    pub fn new(
        id: ObjectId,
        sequence: &[u8; panel_input::PIN_LENGTH],
        policy: PolicySet<'_>,
        tries: u8,
    ) -> Option<Self> {
        if tries == 0 || !sequence.iter().all(|s| matches!(s, 1 | 2 | 4 | 8)) {
            return None;
        }
        let mut data = Zeroizing::new([0; 96]);
        let mut dest = data.as_mut_slice();
        Tlv::new(TAG_POLICY, policy).to_writer(&mut dest).ok()?;
        Tlv::new(TAG_MAX_ATTEMPTS, Be(u16::from(tries)))
            .to_writer(&mut dest)
            .ok()?;
        Tlv::new(TAG_1, id).to_writer(&mut dest).ok()?;
        Tlv::new(TAG_2, sequence.as_slice())
            .to_writer(&mut dest)
            .ok()?;
        let used = 96 - dest.len();
        Some(Self { data, used })
    }
    fn command(&self) -> CommandBuilder<&[u8]> {
        CommandBuilder::new(
            NO_SM_CLA,
            INS_WRITE | INS_AUTH_OBJECT,
            P1_USERID,
            P2_DEFAULT,
            &self.data[..self.used],
            0,
        )
    }
}

impl DataSource for EnrollSequence {
    fn len(&self) -> usize {
        self.command().len()
    }
    fn is_empty(&self) -> bool {
        false
    }
}
impl<W: Writer> DataStream<W> for EnrollSequence {
    fn to_writer(&self, writer: &mut W) -> Result<(), W::Error> {
        self.command().to_writer(writer)
    }
}
impl<W: Writer> Se05XCommand<W> for EnrollSequence {
    type Response<'a> = ();
}

#[cfg(test)]
mod tests {
    use super::*;
    use se05x::se05x::policies::{ObjectAccessRule, ObjectPolicyFlags, Policy};
    #[test]
    fn limited_retries_are_encoded_as_two_bytes_on_applet_seven() {
        let id = ObjectId([0x7d, 0xa0, 0x33, 1]);
        let rules = [Policy {
            object_id: id,
            access_rule: ObjectAccessRule::from_flags(ObjectPolicyFlags::ALLOW_DELETE),
        }];
        let pin = [1; panel_input::PIN_LENGTH];
        let cmd = EnrollSequence::new(id, &pin, PolicySet(&rules), 5).unwrap();
        assert!(cmd.data[..cmd.used]
            .windows(4)
            .any(|w| w == [0x12, 2, 0, 5]));
        assert!(EnrollSequence::new(id, &pin, PolicySet(&rules), 0).is_none());
    }
}

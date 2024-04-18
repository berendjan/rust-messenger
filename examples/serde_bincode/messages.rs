use messenger::traits;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MessageA {
    pub val: u8,
}

impl traits::Message for MessageA {
    type Id = MessageId;
    const ID: MessageId = MessageId::MessageA;
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MessageB {
    pub other_val: u16,
}

impl traits::Message for MessageB {
    type Id = MessageId;
    const ID: MessageId = MessageId::MessageB;
}
#[repr(u16)]
#[derive(PartialEq, Eq, Debug)]
pub enum MessageId {
    MessageA,
    MessageB,
}

impl From<MessageId> for u16 {
    fn from(value: MessageId) -> Self {
        value as u16
    }
}

impl From<u16> for MessageId {
    fn from(value: u16) -> Self {
        match value {
            0 => MessageId::MessageA,
            1 => MessageId::MessageB,
            _ => panic!(),
        }
    }
}

// impl traits::ZeroCopyMessage for MessageA {}
// impl traits::ZeroCopyMessage for MessageB {}

macro_rules! impl_message_traits {
    ($($type:ty),*) => {
        $(
            impl traits::DeserializeFrom for $type {
                fn deserialize_from(buffer: &[u8]) -> Self {
                    bincode::deserialize(buffer).unwrap()
                }
            }

            impl traits::ExtendedMessage for $type {
                fn get_size(&self) -> usize {
                    bincode::serialized_size(self).unwrap() as usize
                }

                fn write_into(&self, buffer: &mut [u8]) {
                    bincode::serialize_into(buffer, self).unwrap();
                }
            }
        )*
    };
}

impl_message_traits!(MessageA, MessageB);

use rust_messenger::traits;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MessageA {
    pub val: u8,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MessageB {
    pub other_val: u16,
}

rust_messenger::messenger_id_enum!(
    MessageId {
        MessageA = 0,
        MessageB = 1,
    }
);

// zero copy
// impl traits::ZeroCopyMessage for MessageA {}
// impl traits::ZeroCopyMessage for MessageB {}

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

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
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);

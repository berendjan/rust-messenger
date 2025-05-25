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

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::core::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

        impl $type {
            pub fn deserialize_from(buffer: &[u8]) -> Self {
                bincode::serde::borrow_decode_from_slice(buffer, bincode::config::standard())
                    .unwrap()
                    .0
            }
        }

        impl traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                bincode::serde::encode_to_vec(self, bincode::config::standard())
                    .unwrap()
                    .len()
            }

            fn write_into(&self, buffer: &mut [u8]) {
                bincode::serde::encode_into_slice(self, buffer, bincode::config::standard())
                    .unwrap();
            }
        }
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);

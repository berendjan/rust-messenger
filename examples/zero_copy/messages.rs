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
            pub fn deserialize_from(buffer: &[u8]) -> &Self
            where
                Self: traits::zero_copy::ZeroCopyMessage,
            {
                let ptr = buffer.as_ptr() as *const Self;
                unsafe { &*ptr }
            }
        }

        impl traits::zero_copy::ZeroCopyMessage for $type {}
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);

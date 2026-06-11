use rust_messenger::traits;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct MessageA {
    pub val: u8,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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
                assert!(
                    buffer.len() >= std::mem::size_of::<Self>(),
                    "buffer too small for message"
                );
                let ptr = buffer.as_ptr() as *const Self;
                assert!(ptr.is_aligned(), "buffer misaligned for message");
                unsafe { &*ptr }
            }
        }

        impl traits::zero_copy::ZeroCopyMessage for $type {}
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);

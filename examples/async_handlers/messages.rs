use rust_messenger::traits;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Request {
    pub request_id: Option<usize>,
    pub val: u8,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Response {
    pub request_id: usize,
    pub other_val: u16,
}

rust_messenger::messenger_id_enum!(
    MessageId {
        Request = 0,
        Response = 1,
    }
);

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::core::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

        impl traits::core::DeserializeFrom for $type {
            fn deserialize_from(buffer: &[u8]) -> Self {
                bincode::deserialize(buffer).unwrap()
            }
        }

        impl traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                bincode::serialized_size(self).unwrap() as usize
            }

            fn write_into(&self, buffer: &mut [u8]) {
                bincode::serialize_into(buffer, self).unwrap();
            }
        }
    };
}

impl_message_traits!(Request, MessageId::Request);
impl_message_traits!(Response, MessageId::Response);

use rust_messenger::traits;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Request {
    pub val: u8,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Response {
    pub response_val: u16,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct IdWrapper<M> {
    pub id: usize,
    pub val: M,
}

rust_messenger::messenger_id_enum!(
    MessageId {
        Request = 0,
        Response = 1,
        RequestWithId = 2,
        ResponseWithId = 3,
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

impl_message_traits!(Request, MessageId::Request);
impl_message_traits!(Response, MessageId::Response);
impl_message_traits!(IdWrapper<Request>, MessageId::RequestWithId);
impl_message_traits!(IdWrapper<Response>, MessageId::ResponseWithId);

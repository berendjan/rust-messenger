use buffa::Message as _;
use rust_messenger::traits;

// Protobuf message types generated from `proto/account.proto` by the buffa
// runtime (the protobuf library used by connect-rust). Vendored so the
// example builds with only `buffa` as a dependency — no protoc at build time.
// See README.md in this directory for how to regenerate.
pub mod account {
    #![allow(clippy::all, dead_code, unused_qualifications)]
    include!("generated/account.rs");
}

pub use account::{GetAccountRequest, GetAccountResponse};

rust_messenger::messenger_id_enum!(
    MessageId {
        GetAccountRequest = 1,
        GetAccountResponse = 2,
    }
);

// Wire the bus message traits onto the buffa-generated types. `connect-rust`
// services would carry exactly these types; here they travel over the
// in-process CircularBus instead of a socket.
macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::core::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

        impl traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                // buffa precomputes the exact wire size; no trial encode.
                self.encoded_len() as usize
            }

            fn write_into(&self, mut buffer: &mut [u8]) {
                // `&mut [u8]` implements `buffa::bytes::BufMut`, so this
                // encodes straight into the bus slot in a single pass.
                self.encode(&mut buffer);
            }
        }

        impl $type {
            pub fn deserialize_from(buffer: &[u8]) -> Self {
                // The bus hands back exactly `get_size()` bytes (the header
                // carries the unpadded length), so the slice is the precise
                // protobuf message — no length prefix or framing needed.
                <$type>::decode_from_slice(buffer).expect("decoding protobuf message failed")
            }
        }
    };
}

impl_message_traits!(GetAccountRequest, MessageId::GetAccountRequest);
impl_message_traits!(GetAccountResponse, MessageId::GetAccountResponse);

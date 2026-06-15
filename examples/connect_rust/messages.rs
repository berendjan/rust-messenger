use buffa::{Message as _, MessageView as _};
use rust_messenger::traits;

// Protobuf message types generated from `proto/account.proto` by the buffa
// runtime (the protobuf library used by connect-rust), with zero-copy view
// types enabled. Vendored so the example builds with only `buffa` as a
// dependency — no protoc at build time. See README.md for how to regenerate.
//
// `account.mod.rs` is the generated glue: it includes the owned types
// (`account.rs`) and, under `__buffa::view`, the borrowed view types
// (`account.__view.rs`), then re-exports the views.
pub mod account {
    #![allow(clippy::all, dead_code, unused_imports, unused_qualifications)]
    include!("generated/account.mod.rs");
}

pub use account::{GetAccountRequest, GetAccountRequestView, GetAccountResponse, GetAccountResponseView};

rust_messenger::messenger_id_enum!(
    MessageId {
        GetAccountRequest = 1,
        GetAccountResponse = 2,
    }
);

// --- Write side: the OWNED types serialize onto the bus -------------------
//
// Handlers send owned messages; ExtendedMessage encodes them into the bus
// slot via buffa (encoded_len + encode).
macro_rules! impl_owned_message {
    ($type:ty, $id:expr) => {
        impl traits::core::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }
        impl traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                self.encoded_len() as usize
            }
            fn write_into(&self, mut buffer: &mut [u8]) {
                // `&mut [u8]` is a `buffa::bytes::BufMut`: encode in one pass.
                self.encode(&mut buffer);
            }
        }
    };
}

impl_owned_message!(GetAccountRequest, MessageId::GetAccountRequest);
impl_owned_message!(GetAccountResponse, MessageId::GetAccountResponse);

// --- Read side: the borrowed VIEW types decode in place -------------------
//
// A view shares its owned type's MessageId (so routing matches the bytes the
// owned type wrote) and `deserialize_from` calls buffa's zero-copy
// `decode_view`: string/bytes fields (`GetAccountResponseView::name`) borrow
// straight from the bus slot — no allocation. The `'a` lifetime ties the view
// to the slot, so it stays valid for the duration of the handler call.
macro_rules! impl_view_message {
    ($view:ident, $id:expr) => {
        impl<'a> traits::core::Message for account::$view<'a> {
            type Id = MessageId;
            const ID: MessageId = $id;
        }
        impl<'a> account::$view<'a> {
            pub fn deserialize_from(buffer: &'a [u8]) -> Self {
                <account::$view<'a>>::decode_view(buffer).expect("decoding protobuf view failed")
            }
        }
    };
}

impl_view_message!(GetAccountRequestView, MessageId::GetAccountRequest);
impl_view_message!(GetAccountResponseView, MessageId::GetAccountResponse);

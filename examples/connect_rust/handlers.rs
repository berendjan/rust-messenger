use crate::config;
use crate::messages;

use rust_messenger::traits;
use rust_messenger::traits::extended::Sender;

rust_messenger::messenger_id_enum! {
    HandlerId {
        Client = 1,
        AccountService = 2,
    }
}

/// Issues one account lookup on start and prints the response. It receives the
/// response as a zero-copy view — `name` borrows straight from the bus slot.
pub struct Client {}

impl Client {
    pub fn new<W: traits::core::Writer>(_config: &config::Config, _: &W) -> Self {
        Client {}
    }
}

impl traits::core::Handler for Client {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::Client;

    fn on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        let request = messages::GetAccountRequest {
            account_id: 42,
            ..Default::default()
        };
        println!("Client -> GetAccountRequest {{ account_id: {} }}", request.account_id);
        Self::send(&request, writer);
    }
}

// Handles the borrowed VIEW, not the owned message: `message.name` is a
// `&str` pointing into the bus buffer, decoded with no allocation.
impl<'a> traits::core::Handle<messages::GetAccountResponseView<'a>> for Client {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::GetAccountResponseView<'a>,
        _writer: &W,
    ) {
        println!(
            "Client <- GetAccountResponseView {{ account_id: {}, name: {:?} (borrowed), balance: {} }}",
            message.account_id, message.name, message.balance
        );
    }
}

/// Answers account lookups. Reads the request as a zero-copy view and replies
/// with an owned response that the bus serializes.
pub struct AccountService {}

impl AccountService {
    pub fn new<W: traits::core::Writer>(_config: &config::Config, _: &W) -> Self {
        AccountService {}
    }
}

impl traits::core::Handler for AccountService {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::AccountService;
}

impl<'a> traits::core::Handle<messages::GetAccountRequestView<'a>> for AccountService {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::GetAccountRequestView<'a>,
        writer: &W,
    ) {
        println!("AccountService <- GetAccountRequestView {{ account_id: {} }}", message.account_id);

        let response = messages::GetAccountResponse {
            account_id: message.account_id,
            name: format!("account-{}", message.account_id),
            balance: 1_000 + message.account_id,
            ..Default::default()
        };
        Self::send(&response, writer);
    }
}

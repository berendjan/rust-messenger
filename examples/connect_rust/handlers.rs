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

/// Issues one account lookup on start and prints the response it gets back.
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

impl traits::core::Handle<messages::GetAccountResponse> for Client {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::GetAccountResponse,
        _writer: &W,
    ) {
        println!(
            "Client <- GetAccountResponse {{ account_id: {}, name: {:?}, balance: {} }}",
            message.account_id, message.name, message.balance
        );
    }
}

/// Answers account lookups, the way a connect-rust service handler would.
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

impl traits::core::Handle<messages::GetAccountRequest> for AccountService {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::GetAccountRequest,
        writer: &W,
    ) {
        println!("AccountService <- GetAccountRequest {{ account_id: {} }}", message.account_id);

        let response = messages::GetAccountResponse {
            account_id: message.account_id,
            name: format!("account-{}", message.account_id),
            balance: 1_000 + message.account_id,
            ..Default::default()
        };
        Self::send(&response, writer);
    }
}

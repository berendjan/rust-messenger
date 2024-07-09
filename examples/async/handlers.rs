use crate::config;
use crate::messages;

use rust_messenger::traits;
use rust_messenger::traits::extended::Sender;

rust_messenger::messenger_id_enum! {
    HandlerId {
        HandlerA = 1,
        HandlerB = 2,
        HandlerC = 3,
    }
}

pub struct HandlerA {}

impl traits::core::Handler for HandlerA {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerA;
    type Config = config::Config;
    fn new(config: &Self::Config) -> Self {
        println!("HandlerA new called with config value \"{}\"", config.value);
        HandlerA {}
    }
}

impl traits::async_traits::AsyncHandler for HandlerA {
    async fn async_on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        println!("HandlerA on_start called");

        Self::send(&messages::MessageB { other_val: 0 }, writer);
    }
}

impl traits::async_traits::AsyncHandle<messages::MessageA> for HandlerA {
    async fn handle<W: traits::core::Writer>(
        message: std::sync::Arc<messages::MessageA>,
        writer: W,
    ) {
        if message.val < 10 {
            println!("received messages::MessageA at HandlerA: {}", message.val);

            let response = messages::MessageB {
                other_val: message.val as u16 + 1,
            };
            Self::send(&response, &writer);
        }
    }
}

pub struct HandlerB {}

impl traits::core::Handler for HandlerB {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerB;
    type Config = config::Config;
    fn new(_: &Self::Config) -> Self {
        HandlerB {}
    }
}

impl traits::async_traits::AsyncHandler for HandlerB {}

impl traits::async_traits::AsyncHandle<messages::MessageB> for HandlerB {
    async fn handle<W: traits::core::Writer>(
        message: std::sync::Arc<messages::MessageB>,
        writer: W,
    ) {
        if message.other_val < 10 {
            println!(
                "received messages::MessageB at HandlerB: {}",
                message.other_val
            );

            let response = messages::MessageA {
                val: message.other_val as u8 + 1,
            };
            Self::send(&response, &writer);
        }
    }
}

pub struct HandlerC {}

impl traits::core::Handler for HandlerC {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerC;
    type Config = config::Config;
    fn new(_: &Self::Config) -> Self {
        HandlerC {}
    }
}

impl traits::async_traits::AsyncHandler for HandlerC {}

impl traits::async_traits::AsyncHandle<messages::MessageA> for HandlerC {
    async fn handle<W: traits::core::Writer>(
        message: std::sync::Arc<messages::MessageA>,
        _writer: W,
    ) {
        println!("received messages::MessageA at HandlerC: {}", message.val)
    }
}

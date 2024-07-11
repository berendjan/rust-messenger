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
    fn new<W: traits::core::Writer>(config: &Self::Config, _: &W) -> Self {
        println!("HandlerA new called with config value \"{}\"", config.value);
        HandlerA {}
    }

    fn on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        println!("HandlerA on_start called");

        Self::send(&messages::MessageB { other_val: 0 }, writer);
    }
}

impl traits::core::Handle<messages::MessageA> for HandlerA {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageA, writer: &W) {
        if message.val < 10 {
            println!("received messages::MessageA at HandlerA: {}", message.val);

            let response = messages::MessageB {
                other_val: message.val as u16 + 1,
            };
            Self::send(&response, writer);
        }
    }
}

pub struct HandlerB {}

impl traits::core::Handler for HandlerB {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerB;
    type Config = config::Config;
    fn new<W: traits::core::Writer>(_: &Self::Config, _: &W) -> Self {
        HandlerB {}
    }
}

impl traits::core::Handle<messages::MessageB> for HandlerB {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageB, writer: &W) {
        if message.other_val < 10 {
            println!(
                "received messages::MessageB at HandlerB: {}",
                message.other_val
            );

            let response = messages::MessageA {
                val: message.other_val as u8 + 1,
            };
            Self::send(&response, writer);
        }
    }
}

pub struct HandlerC {}

impl traits::core::Handler for HandlerC {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerC;
    type Config = config::Config;
    fn new<W: traits::core::Writer>(_: &Self::Config, _: &W) -> Self {
        HandlerC {}
    }
}

impl traits::core::Handle<messages::MessageA> for HandlerC {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageA, _writer: &W) {
        println!("received messages::MessageA at HandlerC: {}", message.val)
    }
}

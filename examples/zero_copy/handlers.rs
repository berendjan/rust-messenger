use crate::config;
use crate::messages;

use rust_messenger::traits;
use rust_messenger::traits::zero_copy::Sender;

rust_messenger::messenger_id_enum! {
    HandlerId {
        HandlerA = 1,
        HandlerB = 2,
        HandlerC = 3,
    }
}

pub struct HandlerA {}

impl HandlerA {
    pub fn new<W: traits::core::Writer>(config: &config::Config, _: &W) -> Self {
        println!("HandlerA new called with config value \"{}\"", config.value);
        HandlerA {}
    }
}

impl traits::core::Handler for HandlerA {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerA;
    fn on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        println!("HandlerA on_start called");

        Self::send::<messages::MessageB, _, _>(writer, |msg| unsafe {
            std::ptr::addr_of_mut!((*msg).other_val).write(0)
        });
    }
}

impl traits::core::Handle<messages::MessageA> for HandlerA {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageA, writer: &W) {
        if message.val < 10 {
            println!("received messages::MessageA at HandlerA: {}", message.val);

            Self::send::<messages::MessageB, _, _>(writer, |msg| unsafe {
                std::ptr::addr_of_mut!((*msg).other_val).write(message.val as u16 + 1)
            });
        }
    }
}

pub struct HandlerB {}

impl HandlerB {
    pub fn new<W: traits::core::Writer>(_: &config::Config, _: &W) -> Self {
        HandlerB {}
    }
}

impl traits::core::Handler for HandlerB {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerB;
}

impl traits::core::Handle<messages::MessageB> for HandlerB {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageB, writer: &W) {
        if message.other_val < 10 {
            println!(
                "received messages::MessageB at HandlerB: {}",
                message.other_val
            );

            Self::send::<messages::MessageA, _, _>(writer, |msg| unsafe {
                std::ptr::addr_of_mut!((*msg).val).write(message.other_val as u8 + 1)
            });
        }
    }
}

pub struct HandlerC {}

impl HandlerC {
    pub fn new<W: traits::core::Writer>(_: &config::Config, _: &W) -> Self {
        HandlerC {}
    }
}

impl traits::core::Handler for HandlerC {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerC;
}

impl traits::core::Handle<messages::MessageA> for HandlerC {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::MessageA, _writer: &W) {
        println!("received messages::MessageA at HandlerC: {}", message.val)
    }
}

use crate::config;
use crate::messages;

use rust_messenger::traits;
use rust_messenger::traits::Sender;

rust_messenger::messenger_id_enum! {
    HandlerId {
        HandlerA = 1,
        HandlerB = 2,
        HandlerC = 3,
    }
}

pub struct HandlerA {}

impl traits::Handler for HandlerA {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerA;
    type Config = config::Config;
    fn new(config: &Self::Config) -> Self {
        println!("HandlerA new called with config value \"{}\"", config.value);
        HandlerA {}
    }

    fn on_start<W: traits::Writer>(&mut self, writer: &mut W) {
        println!("HandlerA on_start called");

        #[cfg(not(feature = "zero_copy"))]
        Self::send(&messages::MessageB { other_val: 0 }, writer);

        #[cfg(feature = "zero_copy")]
        Self::send::<messages::MessageB, _, _>(writer, |msg| unsafe {
            std::ptr::addr_of_mut!((*msg).other_val).write(0)
        });
    }
}

impl traits::Handle<messages::MessageA> for HandlerA {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageA, writer: &W) {
        if message.val < 10 {
            println!("received messages::MessageA at HandlerA: {}", message.val);

            #[cfg(not(feature = "zero_copy"))]
            {
                let response = messages::MessageB {
                    other_val: message.val as u16 + 1,
                };
                Self::send(&response, writer);
            }

            #[cfg(feature = "zero_copy")]
            Self::send::<messages::MessageB, _, _>(writer, |msg| unsafe {
                std::ptr::addr_of_mut!((*msg).other_val).write(message.val as u16 + 1)
            });
        }
    }
}

pub struct HandlerB {}

impl traits::Handler for HandlerB {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerB;
    type Config = config::Config;
    fn new(_: &Self::Config) -> Self {
        HandlerB {}
    }
}

impl traits::Handle<messages::MessageB> for HandlerB {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageB, writer: &W) {
        if message.other_val < 10 {
            println!(
                "received messages::MessageB at HandlerB: {}",
                message.other_val
            );

            #[cfg(not(feature = "zero_copy"))]
            {
                let response = messages::MessageA {
                    val: message.other_val as u8 + 1,
                };
                Self::send(&response, writer);
            }

            #[cfg(feature = "zero_copy")]
            Self::send::<messages::MessageA, _, _>(writer, |msg| unsafe {
                std::ptr::addr_of_mut!((*msg).val).write(message.other_val as u8 + 1)
            });
        }
    }
}

pub struct HandlerC {}

impl traits::Handler for HandlerC {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerC;
    type Config = config::Config;
    fn new(_: &Self::Config) -> Self {
        HandlerC {}
    }
}

impl traits::Handle<messages::MessageA> for HandlerC {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageA, _writer: &W) {
        println!("received messages::MessageA at HandlerC: {}", message.val)
    }
}

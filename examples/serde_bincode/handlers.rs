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
    fn new() -> Self {
        HandlerA {}
    }

    fn on_start<W: traits::Writer>(&mut self, writer: &mut W) {
        println!("HandlerA started");
        Self::send(&messages::MessageB { other_val: 0 }, writer);

        // zero copy
        // Self::send::<messages::MessageB, _, _>(writer, |msg| msg.other_val = 0);
    }
}

impl traits::Handle<messages::MessageA> for HandlerA {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageA, writer: &W) {
        if message.val < 10 {
            let response = messages::MessageB {
                other_val: message.val as u16 + 1,
            };
            println!("received messages::MessageA at HandlerA: {}", message.val);
            Self::send(&response, writer)

            // zero copy
            // Self::send::<messages::MessageB, _, _>(writer, |msg| {
            //     msg.other_val = message.val as u16 + 1
            // });
        }
    }
}

pub struct HandlerB {}

impl traits::Handler for HandlerB {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerB;
    fn new() -> Self {
        HandlerB {}
    }
}

impl traits::Handle<messages::MessageB> for HandlerB {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageB, writer: &W) {
        if message.other_val < 10 {
            let response = messages::MessageA {
                val: message.other_val as u8 + 1,
            };
            println!(
                "received messages::MessageB at HandlerB: {}",
                message.other_val
            );
            Self::send(&response, writer)

            // zero copy
            // Self::send::<messages::MessageA, _, _>(writer, |msg| {
            //     msg.val = message.other_val as u8 + 1
            // });
        }
    }
}

pub struct HandlerC {}

impl traits::Handler for HandlerC {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::HandlerC;
    fn new() -> Self {
        HandlerC {}
    }
}

impl traits::Handle<messages::MessageA> for HandlerC {
    fn handle<W: traits::Writer>(&mut self, message: &messages::MessageA, _writer: &W) {
        println!("received messages::MessageA at HandlerC: {}", message.val)
    }
}

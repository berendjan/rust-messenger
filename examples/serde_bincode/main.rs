mod handlers;
mod messages;

use ::rust_messenger::message_bus;
use ::rust_messenger::Messenger;

Messenger! {
    message_bus::circular_bus::CircularBus<4096>,
    WorkerA:
        handlers: [
            handler_a: handlers::HandlerA,
            handler_b: handlers::HandlerB,
        ]
        routes: [
            handlers::HandlerA, messages::MessageB: [ handler_b ],
            handlers::HandlerB, messages::MessageA: [ handler_a ],
        ]
    WorkerB:
        handlers: [
            handler_c: handlers::HandlerC,
        ]
        routes: [
            handlers::HandlerB, messages::MessageA: [ handler_c ],
        ]
}

pub fn main() {
    let messenger = Messenger::new();
    messenger.run();
}

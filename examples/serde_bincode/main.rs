mod handlers;
mod messages;

use ::rust_messenger::message_bus;
use ::rust_messenger::Messenger;

Messenger! {
    message_bus::atomic_circular_bus::CircularBus<4096>,
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
    let handles = messenger.run();

    std::thread::sleep(std::time::Duration::from_secs(1));
    messenger.stop();
    handles.join();
}

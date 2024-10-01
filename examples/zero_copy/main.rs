mod config;
mod handlers;
mod messages;

// (!) This use statement is required.
use rust_messenger::traits::zero_copy::CastFrom;

rust_messenger::Messenger! {
    config::Config,
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
    let config = config::Config {
        value: "Hello from Config".to_string(),
    };
    let messenger =
        Messenger::new(rust_messenger::message_bus::atomic_circular_bus::CircularBus::new(&config));
    let handles = messenger.run(&config);

    println!("Messenger started, sleeping for 1 millisecond");
    std::thread::sleep(std::time::Duration::from_millis(1));
    messenger.stop();
    handles.join();
}

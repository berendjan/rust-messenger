mod config;
mod handlers;
mod messages;

rust_messenger::Messenger! {
    config::Config,
    rust_messenger::message_bus::atomic_circular_bus::CircularBus,
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
    let messenger = Messenger::new(config);
    let handles = messenger.run();

    println!("Messenger started, sleeping for 1 second");
    std::thread::sleep(std::time::Duration::from_secs(1));
    messenger.stop();
    handles.join();
}

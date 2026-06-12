mod config;
mod handlers;
mod messages;

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
    use rust_messenger::traits::core::Reader;

    let config = config::Config {
        value: "Hello from Config".to_string(),
    };
    let bus = rust_messenger::message_bus::atomic_circular_bus::CircularBus::new(&config);
    let messenger = Messenger::new(bus.clone());
    let handles = messenger.run(&config);

    // Instead of guessing a sleep duration, watch the bus for the end of the
    // ping-pong: the final message is a MessageB carrying 10 (HandlerB stops
    // responding at that value).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut position = 0;
    'wait: while std::time::Instant::now() < deadline {
        while let Some((header, buffer)) = bus.read(position) {
            position += rust_messenger::messenger::ALIGNED_HEADER_SIZE + header.size as usize;
            if header.message_id == u16::from(messages::MessageId::MessageB)
                && messages::MessageB::deserialize_from(buffer).other_val == 10
            {
                break 'wait;
            }
        }
        std::thread::yield_now();
    }

    // Brief grace period so slower workers (HandlerC's printing) catch up
    // before the stop flag ends their loops.
    std::thread::sleep(std::time::Duration::from_millis(10));
    messenger.stop();
    handles.join();
}

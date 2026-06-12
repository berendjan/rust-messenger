mod config;
mod handlers;
mod messages;

// This examples loops back and forth over the CircularBus and an async server & client
// Handler to client to server
// SyncApp -> Request(0) -> AsyncClient
//
// Client opens TCP connection to server and sends request
// AsyncClient -> Request(0) -> AsyncServer
//
// AsyncServer forwards the request to SyncRequestHandler which responds with a new Response object
// AsyncServer -> IdWrapper<Request(0)> -> SyncRequestHandler -> IdWrapper<Response(1)> -> AsyncServer
//
// Server sends the response back over the TCP connection to the client
// AsyncServer -> Response(1) -> AsyncClient
//
// response reaches the app
// AsyncClient -> Response(1) -> SyncApp

rust_messenger::Messenger! {
    config::Config,
    WorkerA:
        handlers: [
            sync_app: handlers::SyncApp,
            async_client: handlers::AsyncClient,
        ]
        routes: [
            handlers::SyncApp, messages::Request: [ async_client ],
            handlers::AsyncClient, messages::Response: [ sync_app ],
        ]
    WorkerB:
        handlers: [
            async_server: handlers::AsyncServer,
            sync_request_handler: handlers::SyncRequestHandler,
        ]
        routes: [
            handlers::AsyncServer, messages::IdWrapper<messages::Request>: [ sync_request_handler ],
            handlers::SyncRequestHandler, messages::IdWrapper<messages::Response>: [ async_server ],
        ]
}

pub fn main() {
    let runtime = std::sync::Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Failed to create runtime"),
    );
    let config = config::Config {
        value: "Hello from Config".to_string(),
        runtime,
        addr: "127.0.0.1:12121".to_string(),
    };
    let bus = rust_messenger::message_bus::atomic_circular_bus::CircularBus::new(&config);
    let messenger = Messenger::new(bus.clone());
    let handles = messenger.run(&config);

    // Instead of guessing a sleep duration, watch the bus until the Response
    // makes it all the way back from the TCP round trip (AsyncClient is the
    // last hop before SyncApp).
    use rust_messenger::traits::core::Reader;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut position = 0;
    'wait: while std::time::Instant::now() < deadline {
        while let Some((header, _)) = bus.read(position) {
            position += rust_messenger::messenger::ALIGNED_HEADER_SIZE + header.size as usize;
            if header.source == u16::from(handlers::HandlerId::AsyncClient)
                && header.message_id == u16::from(messages::MessageId::Response)
            {
                break 'wait;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Brief grace period so WorkerA routes the response to SyncApp before
    // the stop flag ends the worker loops.
    std::thread::sleep(std::time::Duration::from_millis(10));
    messenger.stop();
    handles.join();
}

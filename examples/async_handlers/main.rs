mod config;
mod handlers;
mod messages;

// This examples loops back and forth over the CircularBus and an async server & client
// Handler to client to server
// SyncHandler -> Request(0) -> AsyncClient
//
// over TCP a request
// Async Client -> Request(0) -> AsyncServer
//
// Responding to async server request
// AsyncServer -> Request(0) -> SyncHandler -> Response(1) -> AsyncServer
//
// over TCP a response
// AsyncServer -> Response(1) -> AsyncClient
//
// response reaches handler
// AsyncClient -> Response(2) -> SyncHandler

// (!) This use statement is required.
use rust_messenger::traits::core::DeserializeFrom;

rust_messenger::Messenger! {
    config::Config,
    WorkerA:
        handlers: [
            sync_handler: handlers::SyncHandler,
            async_server: handlers::AsyncServer,
        ]
        routes: [
            handlers::SyncHandler, messages::Response: [ async_server ],
            handlers::AsyncServer, messages::Request: [ sync_handler ],

            handlers::AsyncClient, messages::Response: [ sync_handler ],
        ]
    WorkerB:
        handlers: [
            async_client: handlers::AsyncClient,
        ]
        routes: [
            handlers::SyncHandler, messages::Request: [ async_client ],
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
    let messenger =
        Messenger::new(rust_messenger::message_bus::atomic_circular_bus::CircularBus::new(&config));
    let handles = messenger.run(&config);

    println!("Messenger started, sleeping for 5 milliseconds");
    std::thread::sleep(std::time::Duration::from_millis(5));

    messenger.stop();
    handles.join();
}

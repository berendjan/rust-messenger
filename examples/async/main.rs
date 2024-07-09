mod config;
mod handlers;
mod messages;
mod messenger;

pub fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run())
}

async fn run() {
    let config = config::Config {
        value: "Hello from Config".to_string(),
        threads: 2,
    };
    let messenger = messenger::Messenger::new(config);
    let handles = messenger.run();

    println!("Messenger started, sleeping for 1 millisecond");
    std::thread::sleep(std::time::Duration::from_millis(1));
    messenger.stop();
    handles.join();
}

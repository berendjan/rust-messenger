#[derive(Clone)]
pub struct Config {
    pub value: String,
}

impl rust_messenger::message_bus::atomic_circular_bus::Config for Config {
    const BUFFER_SIZE: usize = 4096;
}

#[derive(Clone)]
pub struct Config {
    pub value: String,
    pub runtime: std::sync::Arc<tokio::runtime::Runtime>,
    pub addr: String,
}

impl rust_messenger::message_bus::atomic_circular_bus::Config for Config {
    fn get_buffer_size(&self) -> usize {
        16384
    }
}

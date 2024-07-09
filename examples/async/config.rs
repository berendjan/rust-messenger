#[derive(Clone)]
pub struct Config {
    pub value: String,
    pub threads: usize,
}

impl rust_messenger::message_bus::atomic_circular_bus::Config for Config {
    fn get_buffer_size(&self) -> usize {
        4096
    }
}

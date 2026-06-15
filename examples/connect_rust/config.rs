#[derive(Clone)]
pub struct Config {}

impl rust_messenger::message_bus::atomic_circular_bus::Config for Config {
    fn get_buffer_size(&self) -> usize {
        16384
    }
}

/// Optional Helper trait that simplifies implementation
/// with serialization / deserialization libraries such as
/// serde or prost.
pub trait ExtendedMessage: super::core::Message {
    fn get_size(&self) -> usize;
    fn write_into(&self, buffer: &mut [u8]);
}

/// Helper trait that provides a serialization default implementation
/// using the functions provided by the ExtendedMessage trait
pub trait Sender {
    fn send<M: ExtendedMessage, W: super::core::Writer>(message: &M, writer: &W);
}

/// Blanket implementation for Handlers.
impl<H: super::core::Handler> Sender for H {
    #[inline]
    fn send<M: ExtendedMessage, W: super::core::Writer>(message: &M, writer: &W) {
        let size = message.get_size();
        writer.write::<M, Self, _>(size, |buffer| {
            message.write_into(buffer);
        });
    }
}

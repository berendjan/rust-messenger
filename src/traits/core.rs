use crate::messenger;

pub trait Handler {
    type Id: Into<u16>;
    const ID: Self::Id;
    fn on_start<W: Writer>(&mut self, _writer: &W) {}
    fn on_loop<W: Writer>(&mut self, _writer: &W) {}
    fn on_stop(&mut self) {}
}

pub trait Handle<M: Message> {
    fn handle<W: Writer>(&mut self, message: &M, writer: &W);
}

pub trait Message {
    type Id: Into<u16>;
    const ID: Self::Id;
}

pub trait Reader {
    /// Returns the header and payload of the message written at `position`,
    /// or `None` if no valid message exists there. The references borrow the
    /// bus and cannot outlive it.
    ///
    /// Implementations with bounded storage may panic if the reader has
    /// fallen so far behind that the message at `position` was overwritten.
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])>;
}

pub trait Writer: Sync + Send + Clone + 'static {
    /// Reserves `size` bytes (rounded up to alignment) and passes the payload
    /// buffer to `callback`. The buffer is only valid inside the callback.
    ///
    /// The first `size` bytes of the buffer are NOT pre-zeroed; the callback
    /// must write every byte it wants readers to see, or stale bytes from
    /// earlier messages may show through (alignment tail bytes beyond `size`
    /// are zeroed by the bus).
    fn write<M: Message, H: Handler, F: FnOnce(&mut [u8])>(&self, size: usize, callback: F);
}

pub trait MessageBus: Reader + Writer {
    fn on_stop(&self) {}
}

pub trait Router {
    fn route<'a, W: Writer>(&mut self, header: &'a messenger::Header, buffer: &'a [u8], writer: &W);
}

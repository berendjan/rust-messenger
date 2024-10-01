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
    fn read<'a>(&self, position: usize) -> Option<(&'a messenger::Header, &'a [u8])>;
}

pub trait Writer: Sync + Send + Clone + 'static {
    fn write<'a, M: Message, H: Handler, F: FnOnce(&'a mut [u8])>(&self, size: usize, callback: F);
}

pub trait MessageBus: Reader + Writer {
    fn on_stop(&self) {}
}

pub trait Router {
    fn route<'a, W: Writer>(&mut self, header: &'a messenger::Header, buffer: &'a [u8], writer: &W);
}

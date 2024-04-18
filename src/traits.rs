use crate::messenger;

pub trait Handler {
    type Id: Into<u16>;
    const ID: Self::Id;
    fn new() -> Self;
    fn on_start<W: Writer>(&mut self, _writer: &mut W) {}
    fn on_loop<W: Writer>(&mut self, _writer: &mut W) {}
}

pub trait Handle<M: Message> {
    fn handle<W: Writer>(&mut self, message: &M, writer: &W);
}

pub trait Message {
    type Id: Into<u16> + From<Self::Id>;
    const ID: Self::Id;
}

pub trait Reader {
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])>;
}

pub(crate) trait InnerWriter {
    fn inner_write<F: FnMut(&mut [u8])>(&self, size: usize, callback: F);
}

pub trait Writer: InnerWriter {
    fn write<M: Message, H: Handler, F: FnMut(&mut [u8])>(&self, size: usize, callback: F);
}

pub trait Stoppable {
    fn stop(&self);
}
impl<MB: MessageBus> Stoppable for MB {
    fn stop(&self) {
        self.inner_write(messenger::ALIGNED_HEADER_SIZE, |buffer| {
            let header_ptr = buffer.as_mut_ptr() as *mut messenger::Header;
            unsafe {
                (*header_ptr).system_flags = messenger::Header::FLAG_STOP;
            }
        });
    }
}

pub trait MessageBus: Reader + Writer + Sync + Send + InnerWriter {}

pub trait Router {
    fn route<W: Writer>(&mut self, header: &messenger::Header, buffer: &[u8], writer: &W);
}

#[cfg(not(feature = "zero_copy"))]
pub trait ExtendedMessage: Message {
    fn get_size(&self) -> usize;
    fn write_into(&self, buffer: &mut [u8]);
}

/// This trait is used to remind you that using the zero copy feature
/// will require you to ensure that each message is trivially copyable.
/// Meaning it should be possible to cast a `mut* u8` type to a `mut* Self`
/// on the buffer.
///
/// Important Note: Rust is non-deterministic in the memory layout of structs.
/// Meaning, if you use the file-backed mmap for replay functionality. You
/// better make sure that all message structs are also `repr(C)` for deterministic
/// memory layout.
///
#[cfg(feature = "zero_copy")]
pub trait ZeroCopyMessage: Message + Sized {}

pub trait DeserializeFrom: Message {
    #[cfg(not(feature = "zero_copy"))]
    fn deserialize_from(buffer: &[u8]) -> Self;

    #[cfg(feature = "zero_copy")]
    fn deserialize_from<'a>(buffer: &'a [u8]) -> &'a Self
    where
        Self: ZeroCopyMessage,
    {
        let ptr = buffer.as_ptr() as *const Self;
        unsafe { &*ptr }
    }
}

pub trait Sender {
    #[cfg(not(feature = "zero_copy"))]
    fn send<M: ExtendedMessage, W: Writer>(message: &M, writer: &W);

    #[cfg(feature = "zero_copy")]
    fn send<M: ZeroCopyMessage, W: Writer, F: FnMut(&mut M)>(writer: &W, callback: F);
}

impl<H: Handler> Sender for H {
    /// Provides the source and message id for the message
    #[cfg(not(feature = "zero_copy"))]
    #[inline]
    fn send<M: ExtendedMessage, W: Writer>(message: &M, writer: &W) {
        let size = message.get_size();
        writer.write::<M, Self, _>(size, |buffer| {
            message.write_into(buffer);
        });
    }

    #[cfg(feature = "zero_copy")]
    #[inline]
    fn send<M: ZeroCopyMessage, W: Writer, F: FnMut(&mut M)>(writer: &W, mut callback: F) {
        let size: usize = std::mem::size_of::<M>();
        writer.write::<M, Self, _>(size, |buffer| {
            let ptr = buffer.as_mut_ptr() as *mut M;
            let message = unsafe { &mut *ptr };
            callback(message);
        });
    }
}

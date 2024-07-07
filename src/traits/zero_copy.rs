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
pub trait ZeroCopyMessage: super::core::Message + Sized {
    const SIZE: usize = std::mem::size_of::<Self>();
}

pub trait CastFrom: super::core::Message {
    fn deserialize_from<'a>(buffer: &'a [u8]) -> &'a Self
    where
        Self: ZeroCopyMessage,
    {
        let ptr = buffer.as_ptr() as *const Self;
        unsafe { &*ptr }
    }
}

pub trait Sender {
    fn send<M: ZeroCopyMessage, W: super::core::Writer, F: FnOnce(*mut M)>(writer: &W, callback: F);
}

impl<H: super::core::Handler> Sender for H {
    #[inline]
    fn send<M: ZeroCopyMessage, W: super::core::Writer, F: FnOnce(*mut M)>(
        writer: &W,
        callback: F,
    ) {
        writer.write::<M, Self, _>(M::SIZE, |buffer| {
            let ptr = buffer.as_mut_ptr() as *mut M;
            callback(ptr);
        });
    }
}

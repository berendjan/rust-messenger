/// Marker for messages that are written and read in place on the bus buffer,
/// i.e. a `*mut u8` into the buffer is cast to a `*mut Self`.
///
/// The `Copy + 'static` bounds keep types that own heap allocations or borrow
/// data (`Box`, `Vec`, `&'a T`, ...) out: reinterpreting buffer bytes as such
/// a type would forge pointers. Types must also not require more than usize
/// alignment; this is enforced at compile time by [`Sender::send`].
///
/// Important Note: Rust is non-deterministic in the memory layout of structs.
/// Meaning, if you use the file-backed mmap for replay functionality. You
/// better make sure that all message structs are also `repr(C)` for deterministic
/// memory layout.
///
pub trait ZeroCopyMessage: super::core::Message + Sized + Copy + 'static {
    const SIZE: usize = std::mem::size_of::<Self>();
}

/// Zero-copy sender: the callback receives a `*mut M` pointing directly into
/// the bus buffer.
///
/// Message types requiring more than usize alignment are rejected at compile
/// time, because the bus only aligns payloads to `size_of::<usize>()`:
///
/// ```compile_fail
/// use rust_messenger::message_bus::atomic_circular_bus::{Config, CircularBus};
/// use rust_messenger::traits;
/// use rust_messenger::traits::zero_copy::Sender;
///
/// #[derive(Clone, Copy)]
/// #[repr(C, align(32))]
/// struct OverAligned { data: [u8; 32] }
/// impl traits::core::Message for OverAligned {
///     type Id = u16;
///     const ID: u16 = 1;
/// }
/// impl traits::zero_copy::ZeroCopyMessage for OverAligned {}
///
/// struct H;
/// impl traits::core::Handler for H {
///     type Id = u16;
///     const ID: u16 = 1;
/// }
///
/// struct C;
/// impl Config for C {
///     fn get_buffer_size(&self) -> usize { 16384 }
/// }
///
/// let bus = CircularBus::new(&C);
/// H::send::<OverAligned, _, _>(&bus, |_msg| {}); // error: alignment too large
/// ```
pub trait Sender {
    fn send<M: ZeroCopyMessage, W: super::core::Writer, F: FnOnce(*mut M)>(writer: &W, callback: F);
}

impl<H: super::core::Handler> Sender for H {
    #[inline]
    fn send<M: ZeroCopyMessage, W: super::core::Writer, F: FnOnce(*mut M)>(
        writer: &W,
        callback: F,
    ) {
        const {
            assert!(
                std::mem::align_of::<M>() <= std::mem::align_of::<usize>(),
                "zero-copy messages must not require more than usize alignment; \
                 the bus only aligns payloads to size_of::<usize>()"
            );
        }
        writer.write::<M, Self, _>(M::SIZE, |buffer| {
            let ptr = buffer.as_mut_ptr() as *mut M;
            callback(ptr);
        });
    }
}

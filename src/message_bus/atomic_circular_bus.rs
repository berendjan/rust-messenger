use crate::messenger;
use crate::mmap::anonymous_mmap;
use crate::traits;

/// A circular bus implementation that uses a shared memory buffer to store messages.
/// The buffer is shared between the writer and the reader.
/// This implementation returns immediately when there is no new message to read.
/// The writer and the reader are lock-free.
///
/// Each message slot is stamped with the absolute position it was written at.
/// `read` validates the stamp, so positions that do not point at the start of
/// a live message (mid-message offsets, or slots already overwritten by a
/// newer lap of the ring) return `None` instead of garbage.
///
/// # Caveats
///
/// * Writers never wait for readers: a reader that falls more than half the
///   buffer behind permanently reads `None` for its stale position. Size the
///   buffer for the worst-case reader lag.
/// * References returned by `read` point into the ring. A writer that laps
///   the ring while they are still in use will overwrite the referenced
///   bytes; copy data out if it must remain stable across writes.
///
/// References returned by `read` cannot outlive the bus:
///
/// ```compile_fail
/// use rust_messenger::message_bus::atomic_circular_bus::{Config, CircularBus};
/// use rust_messenger::traits::core::Reader;
///
/// struct C;
/// impl Config for C {
///     fn get_buffer_size(&self) -> usize { 16384 }
/// }
///
/// let bus = CircularBus::new(&C);
/// let message = bus.read(0);
/// drop(bus);
/// let _ = message; // error: `bus` does not live long enough
/// ```
#[derive(Clone)]
pub struct CircularBus {
    buffer: std::sync::Arc<SharedBuffer>,
}

/// Offset of the slot validity stamp, between the aligned header and the payload.
const STAMP_OFFSET: usize = messenger::ALIGNED_HEADER_SIZE - std::mem::size_of::<u64>();
/// Stamp value marking a slot whose writer has claimed but not yet published it.
const STAMP_INVALID: u64 = u64::MAX;

/// Views the stamp word of the slot starting at `slot` as an atomic.
///
/// SAFETY: caller must ensure `slot` points at a slot with at least
/// `ALIGNED_HEADER_SIZE` accessible bytes; the stamp word is 8-aligned because
/// slots start at `align_to_usize` positions within a page-aligned mapping.
unsafe fn stamp_of<'a>(slot: *const u8) -> &'a std::sync::atomic::AtomicU64 {
    unsafe { &*(slot.add(STAMP_OFFSET) as *const std::sync::atomic::AtomicU64) }
}

/// Publishes a message slot on drop, so the bus stays usable even when a
/// write callback panics: the header is fully written before the callback
/// runs, so readers at worst observe a zeroed payload.
struct PublishGuard<'a> {
    buffer: &'a SharedBuffer,
    ptr: *mut u8,
    position: usize,
    len: usize,
}

impl Drop for PublishGuard<'_> {
    fn drop(&mut self) {
        unsafe { stamp_of(self.ptr) }.store(
            self.position as u64,
            std::sync::atomic::Ordering::Release,
        );

        let new_read_head = self.position + self.len;
        while self
            .buffer
            .read_head
            .compare_exchange_weak(
                self.position,
                new_read_head,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {}
    }
}

pub trait Config {
    fn get_buffer_size(&self) -> usize;
}

struct SharedBuffer {
    mmap: anonymous_mmap::AnonymousMmap,
    write_head: std::sync::atomic::AtomicUsize,
    read_head: std::sync::atomic::AtomicUsize,
    wrap_size: usize,
}

impl CircularBus {
    pub fn new<C: Config>(config: &C) -> CircularBus {
        let mmap = anonymous_mmap::AnonymousMmap::new(config.get_buffer_size())
            .expect("invalid bus buffer size");
        let write_head = std::sync::atomic::AtomicUsize::new(0);
        let read_head = std::sync::atomic::AtomicUsize::new(0);
        let wrap_size = config.get_buffer_size() >> 1;
        Self {
            buffer: std::sync::Arc::new(SharedBuffer {
                mmap,
                write_head,
                read_head,
                wrap_size,
            }),
        }
    }
}

impl traits::core::Writer for CircularBus {
    fn write<M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        let aligned_size = messenger::align_to_usize(size);
        let len = messenger::ALIGNED_HEADER_SIZE + aligned_size;
        assert!(
            len <= self.buffer.wrap_size && aligned_size <= u16::MAX as usize,
            "message of size {size} exceeds the bus capacity ({} payload bytes per message)",
            self.buffer.wrap_size - messenger::ALIGNED_HEADER_SIZE
        );

        let position = self
            .buffer
            .write_head
            .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        let wrapped_pos = position % self.buffer.wrap_size;

        let ptr = self.buffer.mmap.get_ptr() as *mut u8;
        let ptr = unsafe { ptr.add(wrapped_pos) };

        // Invalidate the slot before touching it, so readers of an older lap
        // fail the stamp check instead of observing torn data.
        unsafe { stamp_of(ptr) }.store(STAMP_INVALID, std::sync::atomic::Ordering::Release);

        let hdr_ptr = ptr as *mut messenger::Header;
        unsafe {
            std::ptr::write_bytes(ptr, 0, STAMP_OFFSET);
            std::ptr::write_bytes(ptr.add(messenger::ALIGNED_HEADER_SIZE), 0, aligned_size);
            std::ptr::addr_of_mut!((*hdr_ptr).source).write(H::ID.into());
            std::ptr::addr_of_mut!((*hdr_ptr).message_id).write(M::ID.into());
            std::ptr::addr_of_mut!((*hdr_ptr).size).write(aligned_size as u16);
        }

        // Publishes on drop, even if the callback panics.
        let _publish = PublishGuard {
            buffer: &self.buffer,
            ptr,
            position,
            len,
        };

        let msg_ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts_mut(msg_ptr, aligned_size) };
        callback(buffer);
    }
}

impl traits::core::Reader for CircularBus {
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        let read_head_position = self
            .buffer
            .read_head
            .load(std::sync::atomic::Ordering::Acquire);

        if position >= read_head_position {
            return None;
        }
        let wrapped_position = position % self.buffer.wrap_size;

        let ptr = self.buffer.mmap.get_ptr() as *const u8;
        let ptr = unsafe { ptr.add(wrapped_position) };

        // A slot is only valid for the exact position its writer stamped:
        // mid-message offsets and slots reused by a newer lap fail here.
        let stamp = unsafe { stamp_of(ptr) }.load(std::sync::atomic::Ordering::Acquire);
        if stamp != position as u64 {
            return None;
        }

        let header_ptr = ptr as *const messenger::Header;
        let header = unsafe { &*header_ptr };
        let len = header.size as usize;
        if messenger::ALIGNED_HEADER_SIZE + len > self.buffer.wrap_size {
            return None;
        }

        let ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts(ptr, len) };
        Some((header, buffer))
    }
}

impl traits::core::MessageBus for CircularBus {}

#[cfg(test)]
mod tests {

    use super::*;

    #[derive(Clone, Copy)]
    struct MsgA {
        data: [u16; 5],
    }

    impl traits::core::Message for MsgA {
        type Id = u16;
        const ID: u16 = 2;
    }

    impl traits::extended::ExtendedMessage for MsgA {
        fn get_size(&self) -> usize {
            std::mem::size_of::<Self>()
        }
        fn write_into(&self, buffer: &mut [u8]) {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.data.as_ptr() as *const u8,
                    buffer.as_mut_ptr(),
                    10,
                )
            }
        }
    }

    impl traits::zero_copy::ZeroCopyMessage for MsgA {}

    struct Config {}

    impl super::Config for Config {
        fn get_buffer_size(&self) -> usize {
            16384
        }
    }

    struct HandlerA {}

    impl traits::core::Handler for HandlerA {
        type Id = u16;
        const ID: u16 = 1;
    }

    #[test]
    fn test_circular_bus() {
        use crate::traits::core::Handler;
        use crate::traits::core::Message;
        use crate::traits::core::Reader;
        use crate::traits::extended::Sender;

        let config = Config {};
        let bus = CircularBus::new(&config);
        let mut position: usize = 0;
        for i in 0..500 {
            let message = MsgA {
                data: [i, 1, 2, 3, 4],
            };
            HandlerA::send(&message, &bus);

            let (hdr, buffer) = bus.read(position).unwrap();

            assert_eq!(hdr.source, HandlerA::ID);
            assert_eq!(hdr.message_id, MsgA::ID);
            let expected_size = messenger::align_to_usize(std::mem::size_of::<MsgA>());
            assert_eq!(hdr.size, expected_size as u16);

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += messenger::ALIGNED_HEADER_SIZE + hdr.size as usize;
        }
    }

    /// The slice passed to the write callback must be exactly the (aligned)
    /// payload size. Handing out a longer slice lets safe callback code
    /// overwrite the next slot's header.
    #[test]
    fn test_write_callback_buffer_is_payload_sized() {
        use crate::traits::core::Writer;

        let bus = CircularBus::new(&Config {});
        let size = std::mem::size_of::<MsgA>();
        let expected = messenger::align_to_usize(size);
        bus.write::<MsgA, HandlerA, _>(size, |buffer| {
            assert_eq!(
                buffer.len(),
                expected,
                "callback buffer must not extend past the message payload"
            );
        });
    }

    /// Reading from a position that is not the start of a message must not
    /// reinterpret payload bytes as a Header.
    #[test]
    fn test_read_rejects_mid_message_position() {
        use crate::traits::core::Reader;
        use crate::traits::extended::Sender;

        let bus = CircularBus::new(&Config {});
        let message = MsgA {
            data: [0, 1, 2, 3, 4],
        };
        HandlerA::send(&message, &bus);

        // Position 8 is inside the first slot, not a message start.
        assert!(
            bus.read(8).is_none(),
            "mid-message position must not yield a (garbage) message"
        );
    }

    /// Once the ring wraps, a stale position must not silently return data
    /// from a newer message lap.
    #[test]
    fn test_read_rejects_overwritten_slot() {
        use crate::traits::core::Reader;
        use crate::traits::extended::Sender;

        let bus = CircularBus::new(&Config {});
        // Enough messages to lap the ring several times and rewrite slot 0.
        for i in 0..2048u16 {
            let message = MsgA {
                data: [i, 1, 2, 3, 4],
            };
            HandlerA::send(&message, &bus);
        }

        if let Some((_, buffer)) = bus.read(0) {
            let first = u16::from_ne_bytes([buffer[0], buffer[1]]);
            assert_eq!(
                first, 0,
                "read(0) returned data from a newer lap as if it were message 0"
            );
        }
    }

    /// A message larger than the ring capacity must be rejected instead of
    /// writing past the end of the mapping.
    #[test]
    #[should_panic(expected = "exceeds")]
    fn test_oversized_message_panics() {
        use crate::traits::core::Writer;

        struct SmallConfig {}
        impl super::Config for SmallConfig {
            fn get_buffer_size(&self) -> usize {
                16384
            }
        }

        let bus = CircularBus::new(&SmallConfig {});
        // Larger than wrap_size (buffer_size / 2).
        bus.write::<MsgA, HandlerA, _>(9000, |_| {});
    }

    /// A panicking write callback must not leave the bus in a state where
    /// every subsequent writer spins forever waiting to publish.
    #[test]
    fn test_panicking_writer_does_not_block_bus() {
        use crate::traits::core::Writer;

        let bus = CircularBus::new(&Config {});
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.write::<MsgA, HandlerA, _>(16, |_| panic!("callback failed"));
        }));
        assert!(result.is_err());

        let (tx, rx) = std::sync::mpsc::channel();
        let bus2 = bus.clone();
        std::thread::spawn(move || {
            bus2.write::<MsgA, HandlerA, _>(16, |_| {});
            let _ = tx.send(());
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("writer deadlocked after a previous callback panicked");
    }

    /// The zero-copy sender hands the callback a *mut M into the ring buffer.
    /// The bus guarantees usize alignment for payloads (types needing more
    /// are rejected at compile time, see traits::zero_copy).
    #[test]
    fn test_zero_copy_pointer_is_aligned() {
        use crate::traits::zero_copy::Sender;

        #[derive(Clone, Copy)]
        #[repr(C)]
        struct WordAligned {
            data: u64,
        }
        impl traits::core::Message for WordAligned {
            type Id = u16;
            const ID: u16 = 9;
        }
        impl traits::zero_copy::ZeroCopyMessage for WordAligned {}

        let bus = CircularBus::new(&Config {});
        for _ in 0..3 {
            HandlerA::send::<WordAligned, _, _>(&bus, |msg| {
                assert_eq!(
                    (msg as usize) % std::mem::align_of::<WordAligned>(),
                    0,
                    "zero-copy pointer violates the message type's alignment"
                );
            });
        }
    }

    #[test]
    fn test_zero_copy_circular_bus() {
        use crate::traits::core::Handler;
        use crate::traits::core::Message;
        use crate::traits::core::Reader;
        use crate::traits::zero_copy::Sender;

        let config = Config {};
        let bus = CircularBus::new(&config);
        let mut position: usize = 0;
        for i in 0..500 {
            HandlerA::send::<MsgA, _, _>(&bus, |msg| {
                unsafe { (*msg).data = [i, 1, 2, 3, 4] };
            });

            let (hdr, buffer) = bus.read(position).unwrap();

            assert_eq!(hdr.source, HandlerA::ID);
            assert_eq!(hdr.message_id, MsgA::ID);
            let expected_size = messenger::align_to_usize(std::mem::size_of::<MsgA>());
            assert_eq!(hdr.size, expected_size as u16);

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += messenger::ALIGNED_HEADER_SIZE + hdr.size as usize;
        }
    }
}

use crate::messenger;
use crate::mmap::anonymous_mmap;
use crate::traits;

/// A circular bus implementation that uses a shared memory buffer to store messages.
/// The buffer is shared between the writer and the reader.
/// This implementation returns immediately when there is no new message to read.
/// The writer and the reader are lock-free.
///
/// Publication is per slot: each slot's prefix carries a commit stamp that
/// the writer sets (to `position + 1`) as its final step. `read` returns a
/// message only when the stamp matches the requested position, so writers
/// commit fully independently — a slow writer never delays other writers,
/// and positions that do not point at the start of a committed message
/// (mid-message offsets, in-flight slots) return `None` instead of garbage.
///
/// A write callback that panics leaves its slot uncommitted: other writers
/// are unaffected, but readers stop at that position (in-order consumers
/// cannot skip an unpublished slot). If handlers may panic, run with
/// `panic = "abort"` or keep callbacks trivially infallible.
///
/// # Fallen-behind readers panic
///
/// Writers never wait for readers. A reader that falls more than half the
/// buffer behind has had its messages overwritten; `read` detects this and
/// **panics** ("fell behind") instead of returning torn data or silently
/// skipping messages. Size the buffer for the worst-case reader lag.
///
/// # Caveats
///
/// * The lap check runs when `read` is called. References returned by `read`
///   point into the ring, so a writer that laps the ring *while they are
///   still in use* can overwrite the referenced bytes; the next `read` call
///   will panic, but data already in use may be torn. Keep handler work
///   short relative to buffer capacity, or copy data out.
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

pub trait Config {
    fn get_buffer_size(&self) -> usize;
}

struct SharedBuffer {
    mmap: anonymous_mmap::AnonymousMmap,
    write_head: std::sync::atomic::AtomicUsize,
    wrap_size: usize,
    /// `wrap_size - 1`; valid because the buffer size is a power of two.
    /// Lets the hot path wrap positions with `&` instead of a division.
    wrap_mask: usize,
}

impl CircularBus {
    pub fn new<C: Config>(config: &C) -> CircularBus {
        let mmap = anonymous_mmap::AnonymousMmap::new(config.get_buffer_size())
            .expect("invalid bus buffer size");
        let write_head = std::sync::atomic::AtomicUsize::new(0);
        let wrap_size = config.get_buffer_size() >> 1;
        Self {
            buffer: std::sync::Arc::new(SharedBuffer {
                mmap,
                write_head,
                wrap_size,
                wrap_mask: wrap_size - 1,
            }),
        }
    }
}

impl traits::core::Writer for CircularBus {
    #[inline]
    fn write<M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        let aligned_size = messenger::align_to_usize(size);
        let len = messenger::ALIGNED_HEADER_SIZE + aligned_size;
        assert!(
            len <= self.buffer.wrap_size && size <= u32::MAX as usize,
            "message of size {size} exceeds the bus capacity ({} payload bytes per message)",
            self.buffer.wrap_size - messenger::ALIGNED_HEADER_SIZE
        );

        let position = self
            .buffer
            .write_head
            .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        let wrapped_pos = position & self.buffer.wrap_mask;

        let ptr = self.buffer.mmap.get_ptr() as *mut u8;
        let ptr = unsafe { ptr.add(wrapped_pos) };

        let hdr_ptr = ptr as *mut messenger::Header;
        // Field projection through the raw pointer: borrows only the atomic
        // stamp, so the sibling field writes below do not alias it. Sound on
        // arbitrary slot bytes because every Header bit pattern is valid.
        let stamp = unsafe { &(*hdr_ptr).commit_stamp };

        // Un-commit the slot before mutating it, so readers of an older lap
        // fail the stamp check instead of observing torn data.
        stamp.store(0, std::sync::atomic::Ordering::Release);

        unsafe {
            // Zero the header padding and the alignment tail beyond `size`;
            // the callback is responsible for the payload bytes themselves.
            std::ptr::write_bytes(ptr, 0, std::mem::offset_of!(messenger::Header, commit_stamp));
            std::ptr::write_bytes(
                ptr.add(messenger::ALIGNED_HEADER_SIZE + size),
                0,
                aligned_size - size,
            );
            std::ptr::addr_of_mut!((*hdr_ptr).source).write(H::ID.into());
            std::ptr::addr_of_mut!((*hdr_ptr).message_id).write(M::ID.into());
            // The exact payload length; the padded length the slot occupies is
            // derived from it via Header::aligned_size when walking slots.
            std::ptr::addr_of_mut!((*hdr_ptr).size).write(size as u32);
        }

        // The callback still gets the full padded buffer to write into; the
        // header records that only `size` of it is the real payload.
        let msg_ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts_mut(msg_ptr, aligned_size) };
        callback(buffer);

        // Commit: publish this slot to readers. Writers commit independently;
        // there is no ordering chain between writers. If the callback panicked
        // above, the slot simply stays uncommitted.
        stamp.store(
            messenger::Header::commit_stamp_for(position),
            std::sync::atomic::Ordering::Release,
        );
    }
}

impl CircularBus {
    /// Cold path of [`read`](traits::core::Reader::read): the slot is not
    /// committed for `position`. Decides between "no message yet" (`None`)
    /// and "reader fell behind" (panic). Only here is the write_head cache
    /// line touched, keeping reader polling off the line writers contend on.
    #[cold]
    fn read_uncommitted(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        let write_head_position = self
            .buffer
            .write_head
            .load(std::sync::atomic::Ordering::Relaxed);

        // Writers never wait for readers: once a reservation extends more
        // than wrap_size past `position`, this reader's slot has been handed
        // to a newer message and its own message is gone. Fail loudly.
        assert!(
            position >= write_head_position
                || write_head_position - position <= self.buffer.wrap_size,
            "reader at position {position} fell behind the writers (write head \
             {write_head_position}) and its messages were overwritten; \
             increase the bus buffer size or consume faster"
        );

        None
    }
}

impl traits::core::Reader for CircularBus {
    #[inline]
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        let wrapped_position = position & self.buffer.wrap_mask;

        let ptr = self.buffer.mmap.get_ptr() as *const u8;
        let ptr = unsafe { ptr.add(wrapped_position) };

        let header_ptr = ptr as *const messenger::Header;

        // Fast path: a slot is readable exactly when its writer committed it
        // for this position. In-flight slots, slots whose writer panicked,
        // mid-message offsets, and other laps all fail the comparison. The
        // Acquire load pairs with the writer's Release commit, making the
        // header and payload writes visible. Note this touches only the slot
        // cache line — not write_head, which writers contend on.
        let stamp = unsafe { &(*header_ptr).commit_stamp }
            .load(std::sync::atomic::Ordering::Acquire);
        if stamp != messenger::Header::commit_stamp_for(position) {
            return self.read_uncommitted(position);
        }

        let header = unsafe { &*header_ptr };
        // Validate the padded slot fits; return the exact (unpadded) payload.
        if header.slot_len() > self.buffer.wrap_size {
            return None;
        }

        let ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts(ptr, header.size as usize) };
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
            assert_eq!(hdr.size as usize, std::mem::size_of::<MsgA>());
            assert_eq!(
                hdr.aligned_size(),
                messenger::align_to_usize(std::mem::size_of::<MsgA>())
            );

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += hdr.slot_len();
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

    /// Once the ring wraps past a reader's position, its messages are gone;
    /// the reader must panic loudly instead of receiving newer-lap data.
    #[test]
    #[should_panic(expected = "fell behind")]
    fn test_lapped_reader_panics() {
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

        let _ = bus.read(0);
    }

    /// A reader that is merely behind — but not lapped — still reads its
    /// messages intact.
    #[test]
    fn test_lagging_reader_within_capacity_reads_intact() {
        use crate::traits::core::Reader;
        use crate::traits::extended::Sender;

        let bus = CircularBus::new(&Config {});
        let slot = messenger::ALIGNED_HEADER_SIZE
            + messenger::align_to_usize(std::mem::size_of::<MsgA>());
        // Fill exactly up to capacity: the oldest message is still intact.
        for i in 0..(8192 / slot) {
            let message = MsgA {
                data: [i as u16, 1, 2, 3, 4],
            };
            HandlerA::send(&message, &bus);
        }

        let (_, buffer) = bus.read(0).expect("oldest message should still be readable");
        let first = u16::from_ne_bytes([buffer[0], buffer[1]]);
        assert_eq!(first, 0);
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

    /// Writers commit independently: a panicking write callback leaves its
    /// own slot uncommitted but must not affect any other writer.
    #[test]
    fn test_panicking_writer_does_not_block_bus() {
        use crate::traits::core::Reader;
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

        // The panicked slot stays invisible; the next writer's slot commits.
        assert!(bus.read(0).is_none(), "panicked slot must stay uncommitted");
        let slot = messenger::ALIGNED_HEADER_SIZE + messenger::align_to_usize(16);
        assert!(
            bus.read(slot).is_some(),
            "later writers must commit independently of the panicked one"
        );
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

    /// Maximum-contention stress test: many writers hammer `write_head` with
    /// back-to-back reservations while several independent readers chase the
    /// head, so the `fetch_add` line, the per-slot commit stamps, and the
    /// reader fast path are all contended at once. A barrier releases every
    /// thread simultaneously at the start of each round to force the densest
    /// possible interleaving, and mixed message sizes keep slot boundaries
    /// shifting between laps.
    ///
    /// Each round writes less than `wrap_size` bytes and readers drain fully
    /// before the next round starts, so no reader is ever lapped (across
    /// rounds the ring itself still wraps many times, exercising the
    /// stale-lap stamp rejection). Every reader therefore must observe every
    /// message exactly once, in per-writer order, with every payload byte
    /// and every zeroed alignment tail intact — any torn write, lost commit,
    /// or cross-slot corruption fails an assertion.
    #[test]
    fn test_high_contention_concurrent_writers_and_readers() {
        use crate::traits::core::Handler;
        use crate::traits::core::Message;
        use crate::traits::core::Reader;
        use crate::traits::core::Writer;

        const WRITERS: usize = if cfg!(miri) { 3 } else { 8 };
        const READERS: usize = if cfg!(miri) { 2 } else { 4 };
        const ROUNDS: usize = if cfg!(miri) { 2 } else { 8 };
        const MSGS_PER_WRITER_PER_ROUND: usize = if cfg!(miri) { 20 } else { 750 };
        // Deliberately unaligned sizes so the zeroed alignment tails are
        // exercised; all hold the 16-byte (writer, seq) preamble.
        const SIZES: [usize; 4] = [17, 24, 39, 56];
        const BUFFER_SIZE: usize = 1 << 21;

        // One round must fit in wrap_size, or readers could be lapped and
        // the test would race by design instead of by accident.
        const MAX_SLOT: usize =
            messenger::ALIGNED_HEADER_SIZE + messenger::align_to_usize(SIZES[3]);
        const _: () = assert!(WRITERS * MSGS_PER_WRITER_PER_ROUND * MAX_SLOT <= BUFFER_SIZE >> 1);

        struct BigConfig {}
        impl super::Config for BigConfig {
            fn get_buffer_size(&self) -> usize {
                BUFFER_SIZE
            }
        }

        let bus = CircularBus::new(&BigConfig {});
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS + READERS));
        let mut handles = Vec::new();

        for writer_id in 0..WRITERS {
            let bus = bus.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let mut seq: u64 = 0;
                for _ in 0..ROUNDS {
                    barrier.wait();
                    for _ in 0..MSGS_PER_WRITER_PER_ROUND {
                        let size = SIZES[(seq % SIZES.len() as u64) as usize];
                        bus.write::<MsgA, HandlerA, _>(size, |buf| {
                            buf[..8].copy_from_slice(&(writer_id as u64).to_ne_bytes());
                            buf[8..16].copy_from_slice(&seq.to_ne_bytes());
                            for (i, b) in buf[16..size].iter_mut().enumerate() {
                                *b = (writer_id as u8) ^ (seq as u8) ^ (i as u8);
                            }
                        });
                        seq += 1;
                    }
                    // Wait for readers to drain before the next round can
                    // wrap the ring into this round's slots.
                    barrier.wait();
                }
            }));
        }

        for _ in 0..READERS {
            let bus = bus.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let mut position: usize = 0;
                let mut last_seq = [u64::MAX; WRITERS];
                for _ in 0..ROUNDS {
                    barrier.wait();
                    let mut consumed = 0;
                    let mut spins: u32 = 0;
                    while consumed < WRITERS * MSGS_PER_WRITER_PER_ROUND {
                        let Some((hdr, buf)) = bus.read(position) else {
                            // Mostly spin to stay in the writers' faces, but
                            // yield occasionally so oversubscribed CI runners
                            // still make progress.
                            spins += 1;
                            if spins % 128 == 0 {
                                std::thread::yield_now();
                            } else {
                                std::hint::spin_loop();
                            }
                            continue;
                        };
                        spins = 0;

                        assert_eq!(hdr.source, HandlerA::ID);
                        assert_eq!(hdr.message_id, MsgA::ID);

                        let writer =
                            u64::from_ne_bytes(buf[..8].try_into().unwrap()) as usize;
                        let seq = u64::from_ne_bytes(buf[8..16].try_into().unwrap());
                        assert!(writer < WRITERS, "payload writer id corrupted: {writer}");
                        assert_eq!(
                            seq,
                            last_seq[writer].wrapping_add(1),
                            "writer {writer}: message lost, duplicated or reordered"
                        );
                        last_seq[writer] = seq;

                        // The size is derivable from seq, so a header torn
                        // across slots cannot go unnoticed. `read` returns the
                        // exact unpadded payload; the padded length lives in
                        // aligned_size.
                        let size = SIZES[(seq % SIZES.len() as u64) as usize];
                        assert_eq!(hdr.size as usize, size);
                        assert_eq!(hdr.aligned_size(), messenger::align_to_usize(size));
                        assert_eq!(buf.len(), size);
                        for (i, &b) in buf[16..size].iter().enumerate() {
                            assert_eq!(
                                b,
                                (writer as u8) ^ (seq as u8) ^ (i as u8),
                                "writer {writer} seq {seq}: torn payload at byte {i}"
                            );
                        }

                        position += hdr.slot_len();
                        consumed += 1;
                    }
                    barrier.wait();
                }
                // Drained every round, so every writer's full sequence range
                // must have been observed.
                for (writer, last) in last_seq.iter().enumerate() {
                    assert_eq!(
                        *last as usize + 1,
                        ROUNDS * MSGS_PER_WRITER_PER_ROUND,
                        "writer {writer}: not all messages observed"
                    );
                }
            }));
        }

        for handle in handles {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
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
            assert_eq!(hdr.size as usize, std::mem::size_of::<MsgA>());

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += hdr.slot_len();
        }
    }
}

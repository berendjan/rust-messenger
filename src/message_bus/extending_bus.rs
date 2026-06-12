use crate::messenger;
use crate::mmap::linux::extending_mmap;
use crate::traits;

/// A lock-free, file-backed message bus that grows instead of wrapping.
///
/// Messages are appended to a memory-mapped file whose mapping extends one
/// page at a time (see `ExtendingMmap`): the base address is stable, the full
/// message history stays readable, and readers can replay from any position.
/// Because nothing is ever overwritten, readers cannot be lapped — the
/// fell-behind panic of `CircularBus` does not exist here.
///
/// Publication uses the same per-slot commit stamp as `CircularBus`: writers
/// reserve space with one atomic `fetch_add` and commit with one release
/// store, fully independently of each other. A writer whose slot is not yet
/// backed by mapped pages first grows the file (`fallocate` + `mmap`, one
/// writer at a time); every other write is syscall-free.
///
/// Reopening an existing file resumes appending after the last committed
/// message, while readers can replay the prior history from position 0.
///
/// # Caveats
///
/// * Linux-only (relies on `fallocate`).
/// * Capacity is fixed at `page_size * max_pages` of reserved address space;
///   writing past it panics. History is never reclaimed — size the
///   reservation for the lifetime of the bus.
/// * A write callback that panics leaves an uncommitted hole; readers and
///   the reopen scan stop at the hole.
#[derive(Clone)]
pub struct ExtendingBus {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    mmap: extending_mmap::ExtendingMmap,
    write_head: std::sync::atomic::AtomicUsize,
}

pub trait Config {
    fn get_file_path(&self) -> std::path::PathBuf;
    /// Lower bound for the granularity the file grows by; rounded up to a
    /// power of two of at least the system page size.
    fn get_min_page_len(&self) -> usize;
    /// Maximum number of pages, fixing the total capacity of the bus.
    fn get_max_pages(&self) -> usize;
}

impl ExtendingBus {
    pub fn new<C: Config>(config: &C) -> ExtendingBus {
        use traits::core::Reader;

        let mmap = extending_mmap::ExtendingMmap::new(
            config.get_file_path(),
            config.get_min_page_len(),
            config.get_max_pages(),
        )
        .expect("opening the bus file failed");

        let bus = ExtendingBus {
            inner: std::sync::Arc::new(Inner {
                mmap,
                write_head: std::sync::atomic::AtomicUsize::new(0),
            }),
        };

        // Resume appending after the last committed message of an existing
        // file (a fresh file scans straight to 0).
        let mut end = 0;
        while let Some((header, _)) = bus.read(end) {
            end += messenger::ALIGNED_HEADER_SIZE + header.size as usize;
        }
        bus.inner
            .write_head
            .store(end, std::sync::atomic::Ordering::Relaxed);
        bus
    }
}

impl traits::core::Writer for ExtendingBus {
    #[inline]
    fn write<M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        let aligned_size = messenger::align_to_usize(size);
        let len = messenger::ALIGNED_HEADER_SIZE + aligned_size;
        assert!(
            aligned_size <= u16::MAX as usize,
            "message of size {size} exceeds the maximum message size"
        );

        let position = self
            .inner
            .write_head
            .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        let end = position
            .checked_add(len)
            .expect("bus position overflowed usize");
        assert!(
            end <= self.inner.mmap.reserved_len(),
            "bus file capacity exhausted ({} bytes reserved in pages of {})",
            self.inner.mmap.reserved_len(),
            self.inner.mmap.page_size(),
        );

        // Grow the file until the slot is backed by mapped pages. Only one
        // writer extends at a time; contenders spin on the mapped length.
        // The capacity assert above guarantees this loop terminates.
        while self.inner.mmap.mapped_len() < end {
            match self.inner.mmap.extend() {
                Ok(_) => {}
                Err(extending_mmap::ExtendingMmapError::AlreadyExtending) => {
                    std::hint::spin_loop()
                }
                Err(e) => panic!("extending the bus file failed: {e}"),
            }
        }

        let ptr = unsafe { self.inner.mmap.as_ptr().add(position) };

        let hdr_ptr = ptr as *mut messenger::Header;
        // Field projection through the raw pointer: borrows only the atomic
        // stamp, so the sibling field writes below do not alias it. Sound on
        // arbitrary slot bytes because every Header bit pattern is valid.
        let stamp = unsafe { &(*hdr_ptr).commit_stamp };

        // Un-commit the slot first: an existing file may hold stale bytes
        // here (e.g. appending over a crashed writer's hole).
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
            std::ptr::addr_of_mut!((*hdr_ptr).size).write(aligned_size as u16);
        }

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

impl traits::core::Reader for ExtendingBus {
    #[inline]
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        // Never touch bytes beyond the mapped length: the remainder of the
        // reservation is PROT_NONE. The Acquire load pairs with the Release
        // in extend(), making freshly mapped pages visible.
        let mapped = self.inner.mmap.mapped_len();
        match position.checked_add(messenger::ALIGNED_HEADER_SIZE) {
            Some(slot_end) if slot_end <= mapped => {}
            _ => return None,
        }

        let ptr = unsafe { self.inner.mmap.as_ptr().add(position) };
        let header_ptr = ptr as *const messenger::Header;

        // A slot is only readable once its writer committed it for exactly
        // this position: in-flight slots, slots whose writer panicked, and
        // mid-message offsets all fail here. The Acquire load pairs with the
        // writer's Release commit, making the header and payload visible.
        let stamp = unsafe { &(*header_ptr).commit_stamp }
            .load(std::sync::atomic::Ordering::Acquire);
        if stamp != messenger::Header::commit_stamp_for(position) {
            return None;
        }

        let header = unsafe { &*header_ptr };
        let len = header.size as usize;
        if position + messenger::ALIGNED_HEADER_SIZE + len > mapped {
            return None;
        }

        let ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts(ptr, len) };
        Some((header, buffer))
    }
}

impl traits::core::MessageBus for ExtendingBus {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::core::Reader;
    use crate::traits::extended::Sender;

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

    struct HandlerA {}
    impl traits::core::Handler for HandlerA {
        type Id = u16;
        const ID: u16 = 1;
    }

    /// One MsgA slot: 16 byte header prefix + 16 byte aligned payload.
    const SLOT: usize = messenger::ALIGNED_HEADER_SIZE + 16;

    struct Cfg {
        path: std::path::PathBuf,
        max_pages: usize,
    }

    impl Config for Cfg {
        fn get_file_path(&self) -> std::path::PathBuf {
            self.path.clone()
        }
        fn get_min_page_len(&self) -> usize {
            1 // rounded up to the system page size
        }
        fn get_max_pages(&self) -> usize {
            self.max_pages
        }
    }

    fn temp_cfg(name: &str, max_pages: usize) -> Cfg {
        let path = std::env::temp_dir().join(format!(
            "rust_messenger_extending_bus_{}_{name}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Cfg { path, max_pages }
    }

    fn send(bus: &ExtendingBus, first: u16) {
        let message = MsgA {
            data: [first, 1, 2, 3, 4],
        };
        HandlerA::send(&message, bus);
    }

    fn first_value(buffer: &[u8]) -> u16 {
        u16::from_ne_bytes([buffer[0], buffer[1]])
    }

    #[test]
    #[cfg_attr(miri, ignore)] // file-backed mmap is not supported under Miri
    fn test_write_read_roundtrip() {
        let cfg = temp_cfg("roundtrip", 4);
        let bus = ExtendingBus::new(&cfg);

        send(&bus, 42);
        let (header, buffer) = bus.read(0).expect("message should be readable");
        assert_eq!(header.source, 1);
        assert_eq!(header.message_id, 2);
        assert_eq!(first_value(buffer), 42);

        assert!(bus.read(SLOT).is_none(), "no second message yet");
        assert!(bus.read(8).is_none(), "mid-message position must be None");

        drop(bus);
        std::fs::remove_file(&cfg.path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_grows_across_pages() {
        let cfg = temp_cfg("grow", 4);
        let bus = ExtendingBus::new(&cfg);

        // More than two pages worth of messages (page >= 4096 = 128 slots).
        let count = 300u16;
        for i in 0..count {
            send(&bus, i);
        }

        let mut position = 0;
        for i in 0..count {
            let (_, buffer) = bus.read(position).expect("message should be readable");
            assert_eq!(first_value(buffer), i);
            position += SLOT;
        }
        assert!(bus.read(position).is_none());

        drop(bus);
        std::fs::remove_file(&cfg.path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_reopen_replays_and_resumes() {
        let cfg = temp_cfg("reopen", 4);
        {
            let bus = ExtendingBus::new(&cfg);
            for i in 0..5 {
                send(&bus, i);
            }
        }

        // Reopening replays the existing history...
        let bus = ExtendingBus::new(&cfg);
        let mut position = 0;
        for i in 0..5 {
            let (_, buffer) = bus.read(position).expect("history should replay");
            assert_eq!(first_value(buffer), i);
            position += SLOT;
        }

        // ...and appends after it instead of overwriting.
        send(&bus, 99);
        let (_, buffer) = bus.read(5 * SLOT).expect("appended message readable");
        assert_eq!(first_value(buffer), 99);

        drop(bus);
        std::fs::remove_file(&cfg.path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    #[should_panic(expected = "capacity")]
    fn test_capacity_exhausted_panics() {
        let cfg = temp_cfg("capacity", 1);
        let bus = ExtendingBus::new(&cfg);

        // One message more than fits in the whole reservation.
        let capacity = bus.inner.mmap.reserved_len();
        for i in 0..=(capacity / SLOT) as u16 {
            send(&bus, i);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_concurrent_writers() {
        let cfg = temp_cfg("concurrent", 8);
        let bus = ExtendingBus::new(&cfg);

        let threads = 4;
        let per_thread = 200u16;
        let mut handles = Vec::new();
        for t in 0..threads {
            let bus = bus.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..per_thread {
                    send(&bus, t * per_thread + i);
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        // Every reserved slot must be committed and walkable in order.
        let mut seen = vec![false; (threads * per_thread) as usize];
        let mut position = 0;
        for _ in 0..threads * per_thread {
            let (_, buffer) = bus.read(position).expect("all slots must be committed");
            let value = first_value(buffer) as usize;
            assert!(!seen[value], "message {value} delivered twice");
            seen[value] = true;
            position += SLOT;
        }
        assert!(bus.read(position).is_none());
        assert!(seen.iter().all(|&b| b));

        drop(bus);
        std::fs::remove_file(&cfg.path).unwrap();
    }
}

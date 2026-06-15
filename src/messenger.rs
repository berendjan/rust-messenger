#[repr(C)]
pub struct Header {
    pub source: u16,
    pub message_id: u16,
    /// Exact (unpadded) length of the payload. This is what readers hand to a
    /// deserializer.
    pub size: u16,
    /// Payload length rounded up to alignment — the number of payload bytes
    /// the slot actually occupies. Readers add [`ALIGNED_HEADER_SIZE`] to this
    /// to reach the next slot. Stored (rather than recomputed from `size`) so
    /// the slot stream is self-describing: any reader can walk it without
    /// knowing the alignment rule, including across architectures. Fills the
    /// padding the 8-byte `commit_stamp` would otherwise leave, so the header
    /// stays 16 bytes.
    pub aligned_size: u16,
    /// Publication stamp: 0 while the slot is unwritten or in flight,
    /// [`Header::commit_stamp_for`]`(position)` once the message is
    /// committed. Maintained exclusively by the bus implementations.
    pub(crate) commit_stamp: std::sync::atomic::AtomicU64,
}

impl Header {
    /// Commit value for the slot written at `position`: offset by one so a
    /// zeroed (never-written or in-flight) stamp can never match a real
    /// position.
    pub(crate) const fn commit_stamp_for(position: usize) -> u64 {
        position as u64 + 1
    }

    /// Total bytes this message occupies in the bus: the slot prefix plus the
    /// padded payload. Add this to a position to reach the next message.
    pub fn slot_len(&self) -> usize {
        ALIGNED_HEADER_SIZE + self.aligned_size as usize
    }
}

// Slot positions are aligned to usize, but the atomic commit stamp requires
// u64 alignment; on targets where usize is smaller than u64 the headers
// would be under-aligned, so refuse to compile there.
const _: () = assert!(std::mem::align_of::<Header>() <= std::mem::size_of::<usize>());

pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
/// Size of the per-message slot prefix. The message payload starts at this
/// offset within a slot.
pub const ALIGNED_HEADER_SIZE: usize = align_to_usize(HEADER_SIZE);

/// Aligns to register size of current architecture
pub const fn align_to_usize(from: usize) -> usize {
    const BITS: u32 = std::mem::size_of::<usize>().trailing_zeros();
    ((from + (1 << BITS) - 1) >> BITS) << BITS
}

pub struct JoinHandles {
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl JoinHandles {
    pub fn new(handles: Vec<std::thread::JoinHandle<()>>) -> JoinHandles {
        JoinHandles { handles }
    }

    pub fn join(self) {
        for handle in self.handles {
            handle.join().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_to_usize() {
        assert_eq!(align_to_usize(0), 0);
        assert_eq!(align_to_usize(1), std::mem::size_of::<usize>());
        assert_eq!(align_to_usize(8), std::mem::size_of::<usize>());
        assert_eq!(align_to_usize(9), std::mem::size_of::<usize>() * 2);
        assert_eq!(align_to_usize(16), std::mem::size_of::<usize>() * 2);
        assert_eq!(align_to_usize(17), std::mem::size_of::<usize>() * 3);
    }
}

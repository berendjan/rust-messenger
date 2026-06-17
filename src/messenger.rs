#[repr(C)]
pub struct Header {
    pub source: u16,
    pub message_id: u16,
    /// Exact (unpadded) payload length — what readers hand to a deserializer.
    /// The padded length the slot occupies is derived on demand from this via
    /// [`align_to_usize`] (see [`Header::aligned_size`] / [`Header::slot_len`]);
    /// `align_to_usize` is deterministic on every target the crate compiles for
    /// (64-bit only, enforced below), so the recomputation always agrees. The
    /// `u32` (vs the old `u16`) raises the per-message payload limit from 64 KiB
    /// to whatever the bus ring allows.
    pub size: u32,
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

    /// Padded payload length — the number of payload bytes the slot occupies,
    /// derived from the exact [`Header::size`].
    #[inline]
    pub fn aligned_size(&self) -> usize {
        align_to_usize(self.size as usize)
    }

    /// Total bytes this message occupies in the bus: the slot prefix plus the
    /// padded payload. Add this to a position to reach the next message.
    pub fn slot_len(&self) -> usize {
        ALIGNED_HEADER_SIZE + self.aligned_size()
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

    /// Build a header carrying just a payload length (other fields irrelevant).
    fn header_with_len(size: u32) -> Header {
        Header {
            source: 0,
            message_id: 0,
            size,
            commit_stamp: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[test]
    fn aligned_size_is_derived_from_size() {
        for size in [0u32, 1, 7, 8, 9, 16, 17, 1000, 65_535, 70_000, 1_000_000] {
            let h = header_with_len(size);
            assert_eq!(h.aligned_size(), align_to_usize(size as usize), "aligned for {size}");
            assert_eq!(h.slot_len(), ALIGNED_HEADER_SIZE + align_to_usize(size as usize));
            // Padding is always under one alignment unit.
            assert!((h.aligned_size() - size as usize) < std::mem::size_of::<usize>());
        }
    }

    #[test]
    fn header_is_sixteen_bytes() {
        // u16 source + u16 message_id + u32 size + u64 commit_stamp = 16 bytes.
        assert_eq!(HEADER_SIZE, 16);
    }

    #[test]
    fn size_supports_payloads_past_the_old_u16_limit() {
        // 70_000 bytes would have overflowed the old u16 `size` field.
        assert_eq!(header_with_len(70_000).size, 70_000);
    }
}

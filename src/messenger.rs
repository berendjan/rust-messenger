#[repr(C)]
pub struct Header {
    pub size: u16,
    pub source: u16,
    pub message_id: u16,
}

impl Header {
    pub(crate) const FLAG_STOP: u8 = 0b0000_0001;
}
pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const ALIGNED_HEADER_SIZE: usize = align_to_usize(HEADER_SIZE);

/// Aligns to register size of current architecture
pub const fn align_to_usize(from: usize) -> usize {
    const BITS: u32 = std::mem::size_of::<usize>().trailing_zeros();
    ((from + (1 << BITS) - 1) >> BITS) << BITS
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

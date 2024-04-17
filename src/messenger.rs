#[repr(C)]
#[derive(Debug)]
pub struct Header {
    pub size: u16,
    pub source: u16,
    pub message_id: u16,
}
pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const ALIGNED_HEADER_SIZE: usize = align_to_usize(HEADER_SIZE);

/// Aligns to register size of current architecture
pub const fn align_to_usize(from: usize) -> usize {
    const BITS: u32 = std::mem::size_of::<usize>().trailing_zeros();
    (((from - 1) >> BITS) + 1) << BITS
}

use libc::c_int;

/// Options the memory map manager is created with
#[derive(Copy, Clone)]
pub enum ManagerOption {}

/// Options the memory map is created with
#[derive(Copy, Clone)]
pub enum MapOption {
    /// The memory should be readable
    MapReadable,
    /// The memory should be writable
    MapWritable,
    /// The memory should be executable
    MapExecutable,
    /// Create a map for a specific address range. Corresponds to `MAP_FIXED` on
    /// POSIX.
    MapAddr(*const u8),
    /// Create a memory mapping for a file with a given fd.
    MapFd(c_int),
    /// When using `MapFd`, the start of the map is `usize` bytes from the start
    /// of the file.
    MapOffset(usize),
    /// On POSIX, this can be used to specify the default flags passed to
    /// `mmap`. By default it uses `MAP_PRIVATE` and, if not using `MapFd`,
    /// `MAP_ANON`. This will override both of those. This is platform-specific
    /// (the exact values used) and ignored on Windows.
    MapNonStandardFlags(c_int),
}

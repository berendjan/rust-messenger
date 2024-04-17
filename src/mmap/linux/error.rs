use std::error::Error;
use std::fmt;
use std::io;
use ManagerError::*;

#[derive(Copy, Clone, Debug)]
pub enum ManagerError {
    /// min length must be a positive integer
    ErrZeroLength,
    /// the filename is invalid
    ErrInvalidFilename(&'static str),
    /// Could not reserve address space
    ErrReserveMemory,
    /// File size is not multiple of page size
    ErrFileSize,
    /// Max pages reached
    ErrMaxPages,
    /// Memory map is already being extended
    ErrIsExtending,
    /// Error opening file
    ErrOpenFile,
    /// Error allocating memory
    ErrAllocate,
    /// Error remapping mmap
    ErrRemap,
    /// Overflow error
    ErrOverflow,
}

impl fmt::Display for ManagerError {
    fn fmt(&self, out: &mut fmt::Formatter) -> fmt::Result {
        let str = match *self {
            ErrInvalidFilename(err) => return write!(out, "Invalid filename error: {}", err),
            ErrZeroLength => "Minimum length must be greater than zero",
            ErrReserveMemory => "Could not reserve memory for all pages",
            ErrFileSize => "File size is not multiple of page size",
            ErrMaxPages => "Maximum number of pages reached",
            ErrIsExtending => "Memory map is already extending",
            ErrOpenFile => "Error while opening mmap file",
            ErrAllocate => "Error allocating memory to mmap",
            ErrRemap => "Error performing mmap",
            ErrOverflow => "Error overflowing variable",
        };
        write!(out, "{}", str)
    }
}

impl From<io::Error> for ManagerError {
    fn from(_err: io::Error) -> Self {
        ManagerError::ErrInvalidFilename(stringify!("{}", _err))
    }
}

impl Error for ManagerError {
    fn description(&self) -> &str {
        "memory map manager error"
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MapError {
    /// fd was not open for reading or, if using `MapWritable`, was not open for
    /// writing.
    ErrFdNotAvail,
    /// fd was not valid
    ErrInvalidFd,
    /// Either the address given by `MapAddr` or offset given by `MapOffset` was
    /// not a multiple of `MemoryMap::granularity` (unaligned to page size).
    ErrUnaligned,
    /// With `MapFd`, the fd does not support mapping.
    ErrNoMapSupport,
    /// If using `MapAddr`, the address + `min_len` was outside of the process's
    /// address space. If using `MapFd`, the target of the fd didn't have enough
    /// resources to fulfill the request.
    ErrNoMem,
    /// A zero-length map was requested. This is invalid according to
    /// [POSIX](http://pubs.opengroup.org/onlinepubs/9699919799/functions/mmap.html).
    /// Not all platforms obey this, but this wrapper does.
    ErrZeroLength,
    /// Unrecognized error. The inner value is the unrecognized errno.
    ErrUnknown(isize),
}

impl fmt::Display for MapError {
    fn fmt(&self, out: &mut fmt::Formatter) -> fmt::Result {
        let str = match *self {
            MapError::ErrFdNotAvail => "fd not available for reading or writing",
            MapError::ErrInvalidFd => "Invalid fd",
            MapError::ErrUnaligned => {
                "Unaligned address, invalid flags, negative length or \
                 unaligned offset"
            }
            MapError::ErrNoMapSupport => "File doesn't support mapping",
            MapError::ErrNoMem => "Invalid address, or not enough available memory",
            MapError::ErrZeroLength => "Zero-length mapping not allowed",
            MapError::ErrUnknown(code) => return write!(out, "Unknown error = {}", code),
        };
        write!(out, "{}", str)
    }
}

impl Error for MapError {
    fn description(&self) -> &str {
        "memory map error"
    }
}

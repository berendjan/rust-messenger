use std::os::unix::io::AsRawFd;
use std::path::Path;

/// A file-backed memory mapping that can grow in place.
///
/// On creation, a large contiguous region of `page_size * max_pages` bytes is
/// reserved as `PROT_NONE`, so the base address is stable for the lifetime of
/// the mapping. [`extend`](Self::extend) allocates one more `page_size` chunk
/// in the backing file (`fallocate`, so the blocks exist and later writes
/// cannot SIGBUS on a full disk) and overlays it onto the reservation with
/// `MAP_FIXED`. Readers holding pointers into already-mapped pages are never
/// invalidated by an extension.
///
/// Extension takes `&self` and is guarded by an atomic flag, so the mapping
/// can be shared in an `Arc` between writers (which extend) and readers.
/// `mapped_len` grows monotonically and is published with `Release`; readers
/// observing it with `Acquire` may touch every byte below it.
///
/// Deliberately not `Clone`: the struct owns the reservation and unmaps it on
/// drop. Share it through an `Arc` instead.
pub struct ExtendingMmap {
    file: std::fs::File,
    address_begin: *mut u8,
    reserved_bytes: usize,
    page_size: usize,
    mapped_bytes: std::sync::atomic::AtomicUsize,
    is_extending: std::sync::atomic::AtomicBool,
}

// SAFETY: ExtendingMmap is the unique owner of its reservation; the raw
// pointer is only an address. Growth is synchronized by `is_extending` and
// the Release/Acquire pair on `mapped_bytes`; synchronization of the mapped
// bytes themselves is the responsibility of the users of `as_ptr`.
unsafe impl Send for ExtendingMmap {}
unsafe impl Sync for ExtendingMmap {}

#[derive(Debug)]
pub enum ExtendingMmapError {
    /// `min_len` and `max_pages` must both be non-zero.
    ZeroLength,
    /// `page_size * max_pages` does not fit in usize.
    Overflow,
    /// The reservation is fully mapped; the file cannot grow further.
    MaxPagesReached,
    /// Another extension is in progress.
    AlreadyExtending,
    /// An existing backing file's size is not a multiple of the page size.
    FileSizeNotPageMultiple { file_len: u64, page_size: usize },
    /// Filesystem operation failed (open, create_dir_all, metadata, fallocate).
    Io(std::io::Error),
    /// mmap itself failed.
    Mmap(std::io::Error),
}

impl std::fmt::Display for ExtendingMmapError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ZeroLength => write!(f, "min_len and max_pages must be non-zero"),
            Self::Overflow => write!(f, "page_size * max_pages overflows usize"),
            Self::MaxPagesReached => write!(f, "maximum number of pages reached"),
            Self::AlreadyExtending => write!(f, "another extension is already in progress"),
            Self::FileSizeNotPageMultiple {
                file_len,
                page_size,
            } => write!(
                f,
                "backing file size {file_len} is not a multiple of the page size {page_size}"
            ),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Mmap(err) => write!(f, "mmap failed: {err}"),
        }
    }
}

impl std::error::Error for ExtendingMmapError {}

impl From<std::io::Error> for ExtendingMmapError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl ExtendingMmap {
    /// Opens (or creates) `path` and reserves address space for up to
    /// `max_pages` pages of `align_page_size(min_len)` bytes each. An existing
    /// file is mapped in full; a new or empty file is extended to one page.
    pub fn new(
        path: impl AsRef<Path>,
        min_len: usize,
        max_pages: usize,
    ) -> Result<ExtendingMmap, ExtendingMmapError> {
        let path = path.as_ref();
        if min_len == 0 || max_pages == 0 {
            return Err(ExtendingMmapError::ZeroLength);
        }
        let page_size = align_page_size(min_len)?;
        let reserved_bytes = page_size
            .checked_mul(max_pages)
            .ok_or(ExtendingMmapError::Overflow)?;

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let file_len = file.metadata()?.len();
        if file_len % page_size as u64 != 0 {
            return Err(ExtendingMmapError::FileSizeNotPageMultiple {
                file_len,
                page_size,
            });
        }
        if file_len > reserved_bytes as u64 {
            return Err(ExtendingMmapError::MaxPagesReached);
        }

        let address_begin = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                reserved_bytes,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if address_begin == libc::MAP_FAILED {
            return Err(ExtendingMmapError::Mmap(std::io::Error::last_os_error()));
        }

        let mmap = ExtendingMmap {
            file,
            address_begin: address_begin as *mut u8,
            reserved_bytes,
            page_size,
            mapped_bytes: std::sync::atomic::AtomicUsize::new(0),
            is_extending: std::sync::atomic::AtomicBool::new(false),
        };

        // From here on, any error path unmaps the reservation via Drop.
        if file_len > 0 {
            mmap.map_range(0, file_len as usize)?;
            mmap.mapped_bytes
                .store(file_len as usize, std::sync::atomic::Ordering::Release);
        } else {
            mmap.extend()?;
        }
        Ok(mmap)
    }

    /// Grows the backing file and the mapping by one page. Returns the new
    /// mapped length. Already-mapped pages and the base address are
    /// unaffected, so concurrent readers are never invalidated.
    pub fn extend(&self) -> Result<usize, ExtendingMmapError> {
        if self
            .is_extending
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            return Err(ExtendingMmapError::AlreadyExtending);
        }
        let result = self.extend_locked();
        self.is_extending
            .store(false, std::sync::atomic::Ordering::Release);
        result
    }

    fn extend_locked(&self) -> Result<usize, ExtendingMmapError> {
        // Only the thread holding `is_extending` writes `mapped_bytes`.
        let old_len = self.mapped_bytes.load(std::sync::atomic::Ordering::Relaxed);
        let new_len = old_len + self.page_size;
        if new_len > self.reserved_bytes {
            return Err(ExtendingMmapError::MaxPagesReached);
        }

        let result = unsafe {
            libc::fallocate(
                self.file.as_raw_fd(),
                0,
                old_len as libc::off_t,
                self.page_size as libc::off_t,
            )
        };
        if result < 0 {
            return Err(ExtendingMmapError::Io(std::io::Error::last_os_error()));
        }

        self.map_range(old_len, self.page_size)?;
        self.mapped_bytes
            .store(new_len, std::sync::atomic::Ordering::Release);
        Ok(new_len)
    }

    /// Maps `len` bytes of the file at `offset` onto the reservation at the
    /// same offset. `MAP_FIXED` either maps exactly at the requested address
    /// or fails.
    fn map_range(&self, offset: usize, len: usize) -> Result<(), ExtendingMmapError> {
        let addr = unsafe { self.address_begin.add(offset) };
        let mapped = unsafe {
            libc::mmap(
                addr as *mut libc::c_void,
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED_VALIDATE | libc::MAP_FIXED,
                self.file.as_raw_fd(),
                offset as libc::off_t,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(ExtendingMmapError::Mmap(std::io::Error::last_os_error()));
        }
        debug_assert_eq!(mapped as *mut u8, addr);
        Ok(())
    }

    /// Base address of the mapping; stable for the lifetime of self.
    /// Only the first [`mapped_len`](Self::mapped_len) bytes are accessible.
    pub fn as_ptr(&self) -> *mut u8 {
        self.address_begin
    }

    /// Currently mapped (and file-backed) length in bytes.
    pub fn mapped_len(&self) -> usize {
        self.mapped_bytes.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Total reserved address space; `mapped_len` can grow up to this.
    pub fn reserved_len(&self) -> usize {
        self.reserved_bytes
    }

    /// Bytes added per extension.
    pub fn page_size(&self) -> usize {
        self.page_size
    }
}

impl Drop for ExtendingMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.address_begin as *mut libc::c_void, self.reserved_bytes);
        }
    }
}

/// Returns the page size of the current system.
fn system_page_size() -> usize {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page_size > 0, "sysconf(_SC_PAGESIZE) failed");
    page_size as usize
}

/// Rounds `min_len` up to the next power of two that is at least the system
/// page size.
fn align_page_size(min_len: usize) -> Result<usize, ExtendingMmapError> {
    debug_assert!(min_len > 0);
    let target = (min_len - 1) | (system_page_size() - 1);
    if target.leading_zeros() == 0 {
        return Err(ExtendingMmapError::Overflow);
    }
    Ok(1 << (usize::BITS - target.leading_zeros()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cloning would duplicate the owning pointer and munmap the reservation
    // twice (use-after-free for the surviving handle).
    static_assertions::assert_not_impl_any!(ExtendingMmap: Clone);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rust_messenger_extending_mmap_{}_{name}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn test_align_page_size() {
        let page = system_page_size();
        assert_eq!(align_page_size(1).unwrap(), page);
        assert_eq!(align_page_size(page).unwrap(), page);
        assert_eq!(align_page_size(page + 1).unwrap(), page << 1);
        assert_eq!(align_page_size(10_000).unwrap(), 1 << 14);
    }

    #[test]
    fn test_invalid_params_are_err() {
        assert!(matches!(
            ExtendingMmap::new("/tmp/unused", 0, 1),
            Err(ExtendingMmapError::ZeroLength)
        ));
        assert!(matches!(
            ExtendingMmap::new("/tmp/unused", 1, 0),
            Err(ExtendingMmapError::ZeroLength)
        ));
        assert!(matches!(
            ExtendingMmap::new("/tmp/unused", usize::MAX, 2),
            Err(ExtendingMmapError::Overflow)
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // file-backed mmap is not supported under Miri
    fn test_new_maps_one_page_and_is_writable() {
        let path = temp_path("new");
        let mmap = ExtendingMmap::new(&path, 1, 4).unwrap();
        assert_eq!(mmap.mapped_len(), mmap.page_size());
        assert_eq!(mmap.reserved_len(), mmap.page_size() * 4);

        unsafe {
            mmap.as_ptr().write(0xAB);
            assert_eq!(mmap.as_ptr().read(), 0xAB);
        }
        drop(mmap);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extend_keeps_address_and_data() {
        let path = temp_path("extend");
        let mmap = ExtendingMmap::new(&path, 1, 4).unwrap();
        let base = mmap.as_ptr();
        let page = mmap.page_size();
        unsafe { base.write(0xCD) };

        assert_eq!(mmap.extend().unwrap(), 2 * page);
        assert_eq!(mmap.as_ptr(), base, "base address must be stable");
        assert_eq!(unsafe { base.read() }, 0xCD, "old data must survive");
        // The new page is mapped, zero-filled, and writable.
        unsafe {
            assert_eq!(base.add(page).read(), 0);
            base.add(2 * page - 1).write(0xEF);
        }
        // A second extension proves the is_extending flag resets.
        assert_eq!(mmap.extend().unwrap(), 3 * page);

        drop(mmap);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extend_stops_at_max_pages() {
        let path = temp_path("max_pages");
        let mmap = ExtendingMmap::new(&path, 1, 2).unwrap();
        mmap.extend().unwrap();
        assert!(matches!(
            mmap.extend(),
            Err(ExtendingMmapError::MaxPagesReached)
        ));

        drop(mmap);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_reopen_maps_existing_content() {
        let path = temp_path("reopen");
        {
            let mmap = ExtendingMmap::new(&path, 1, 4).unwrap();
            mmap.extend().unwrap();
            unsafe { mmap.as_ptr().write_bytes(0x42, 16) };
        }

        let reopened = ExtendingMmap::new(&path, 1, 4).unwrap();
        assert_eq!(reopened.mapped_len(), reopened.page_size() * 2);
        for i in 0..16 {
            assert_eq!(unsafe { reopened.as_ptr().add(i).read() }, 0x42);
        }

        drop(reopened);
        std::fs::remove_file(&path).unwrap();
    }
}

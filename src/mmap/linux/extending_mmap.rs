use super::error::ManagerError;
use libc::c_void;
use std::fs;
use std::mem::size_of;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

struct MemoryMapManager {
    filename: String,
    page_size: usize,
    address_begin: *mut u8,
    next_page_offset: usize,
    is_extending: AtomicBool,
}

impl MemoryMapManager {
    pub fn new(
        filename: &str,
        min_len: usize,
        max_pages: usize,
    ) -> Result<MemoryMapManager, ManagerError> {
        let path = Path::new(filename);

        if path.is_dir() {
            Err(ManagerError::ErrInvalidFilename(&"Filename is a directory"))?
        }

        match path.parent() {
            Some(dir) => fs::create_dir_all(dir)?,
            None => Err(ManagerError::ErrInvalidFilename(&"Directory invalid"))?,
        };

        if min_len == 0 {
            Err(ManagerError::ErrZeroLength)?
        }

        let page_size = align_page_size(min_len);

        let total_bytes = match page_size.checked_mul(max_pages) {
            Some(bytes) => bytes,
            None(_) => Err(ManagerError::ErrOverflow)?,
        };

        let mut addr: *const u8 = ptr::null();
        let prot = libc::PROT_NONE;
        let flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE;

        let address_begin =
            unsafe { libc::mmap(addr as *mut c_void, total_bytes, prot, flags, -1, 0) };
        if address_begin == libc::MAP_FAILED {
            Err(ManagerError::ErrReserveMemory)?
        }

        let mut manager = MemoryMapManager {
            filename: filename.to_string(),
            page_size: page_size,
            address_begin: address_begin as *mut u8,
            next_page_offset: 0,
            is_extending: AtomicBool::new(false),
        };

        if !manager.try_open_file(&path) {
            manager.extend_mmap();
        };

        Ok(manager)
    }

    pub fn extend_mmap(&mut self) -> Result<&MemoryMapManager, ManagerError> {
        if let Err(_) = self.is_extending.compare_exchange_weak(
            false,
            true,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Err(ManagerError::ErrIsExtending)?
        }

        let oflag = libc::O_RDWR;
        let mode_t = libc::S_IRUSR | libc::S_IWUSR;
        let fd = unsafe { libc::open(self.filename.as_ptr() as *const i8, oflag, mode_t) };

        if fd < 0 {
            Err(ManagerError::ErrOpenFile)?
        }

        let result = unsafe {
            libc::fallocate(
                fd,
                0,
                self.next_page_offset as libc::off_t,
                self.page_size as libc::off_t,
            )
        };

        if result < 0 {
            Err(ManagerError::ErrAllocate)?
        }

        let prot = libc::PROT_READ | libc::PROT_WRITE;
        let flag = libc::MAP_SHARED_VALIDATE | libc::MAP_FIXED;
        let len = self.next_page_offset + self.page_size;
        let address =
            unsafe { libc::mmap(self.address_begin as *mut c_void, len, prot, flag, fd, 0) };

        if self.address_begin != address as *mut u8 {
            Err(ManagerError::ErrRemap)?
        }

        unsafe { libc::close(fd) };

        let dst = unsafe { self.address_begin.offset(len as isize) };
        unsafe { libc::memset(dst as *mut c_void, 0, self.page_size) };

        self.next_page_offset += self.page_size;

        Ok(self)
    }

    fn try_open_file(&mut self, path: &Path) -> bool {
        fs::OpenOptions::new().open(&path).map_or_else(
            |e| false,
            |f| {
                f.metadata().map_or_else(
                    |e| false,
                    |metadata| {
                        self.next_page_offset = metadata.len() as usize;
                        true
                    },
                )
            },
        )
    }
}

/// Retrieve last OS error
fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}

/// Returns page size of current architecture
fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

/// Aligns to register size of current architecture
fn _align_to_usize(from: usize) -> usize {
    const BITS: u32 = size_of::<usize>().trailing_zeros();
    (((from - 1) >> BITS) + 1) << BITS
}

/// Aligns page size to exponent of 2 and with minimum size equal to page size.
fn align_page_size(min_len: usize) -> usize {
    const USIZE_BITS: usize = size_of::<usize>() * 8;
    let leading_zeros: usize = ((min_len - 1) | (page_size() - 1)).leading_zeros() as usize;
    1 << (USIZE_BITS - leading_zeros)
}

#[cfg(test)]
mod tests {

    use crate::mmap::{
        error::ManagerError,
        manager::{page_size, MemoryMapManager},
        option::ManagerOption,
    };

    #[test]
    fn test_align_to_usize() {
        use crate::mmap::manager::_align_to_usize;
        use std::mem::size_of;

        assert_eq!(_align_to_usize(1), size_of::<usize>());
        assert_eq!(_align_to_usize(size_of::<usize>()), size_of::<usize>());
        assert_eq!(
            _align_to_usize(size_of::<usize>() + 1),
            size_of::<usize>() << 1
        );
    }

    #[test]
    fn test_align_page_size() {
        use crate::mmap::manager::align_page_size;

        let sc_pagesize = page_size();
        assert_eq!(align_page_size(1), sc_pagesize);
        assert_eq!(align_page_size(sc_pagesize), sc_pagesize);
        assert_eq!(align_page_size(sc_pagesize + 1), sc_pagesize << 1);
        assert_eq!(align_page_size(10_000), 1 << 14);
    }

    #[test]
    fn test_dir() {
        let tmp_dir = tempdir::TempDir::new("").unwrap();

        let file_path = tmp_dir.path().join("mmap.log");

        let filename = file_path.to_str().unwrap();

        let min_len = page_size() * 2;

        let max_pages = 2;

        let manager = match MemoryMapManager::new(filename, min_len, max_pages) {
            Ok(man) => man,
            Err(err) => panic!("{:?}", err),
        };
    }
}

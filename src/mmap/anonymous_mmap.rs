use std::ffi::c_void;

/// An anonymous memory mapping.
///
/// Backed by `mmap(MAP_ANON | MAP_PRIVATE)` on Unix and
/// `VirtualAlloc(MEM_RESERVE | MEM_COMMIT)` on Windows; both hand back
/// page-aligned, zero-initialized, read-write memory.
///
/// Deliberately not `Clone`: the struct owns the mapping and unmaps it on
/// drop, so a second handle would leave the first one dangling. Share it
/// through an `Arc` instead.
pub struct AnonymousMmap {
    ptr: *mut c_void,
    len: usize,
}

// SAFETY: AnonymousMmap is a unique owner of its mapping; the raw pointer is
// only an address, and all synchronization of the memory behind it is the
// responsibility of the (atomic-based) users of `get_ptr`.
unsafe impl Sync for AnonymousMmap {}
unsafe impl Send for AnonymousMmap {}

impl AnonymousMmap {
    pub fn new(len: usize) -> Result<Self, std::io::Error> {
        let page_size = platform::page_size();
        if len == 0 || len & (len - 1) != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("len must be a power of 2, got {len}"),
            ));
        }
        if len < page_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("len must be at least the page size: {len} < {page_size}"),
            ));
        }

        let ptr = platform::map_anonymous(len)?;
        Ok(Self { ptr, len })
    }

    pub fn get_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for AnonymousMmap {
    fn drop(&mut self) {
        unsafe { platform::unmap(self.ptr, self.len) }
    }
}

#[cfg(unix)]
mod platform {
    use std::ffi::c_void;

    pub fn page_size() -> usize {
        unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) as usize }
    }

    pub fn map_anonymous(len: usize) -> Result<*mut c_void, std::io::Error> {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(ptr)
    }

    /// SAFETY: `ptr`/`len` must come from a successful `map_anonymous` call,
    /// and the mapping must not be used afterwards.
    pub unsafe fn unmap(ptr: *mut c_void, len: usize) {
        unsafe {
            libc::munmap(ptr, len);
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
    };
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    pub fn page_size() -> usize {
        let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut info) };
        info.dwPageSize as usize
    }

    pub fn map_anonymous(len: usize) -> Result<*mut c_void, std::io::Error> {
        let ptr =
            unsafe { VirtualAlloc(std::ptr::null(), len, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) };

        if ptr.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(ptr)
    }

    /// SAFETY: `ptr` must come from a successful `map_anonymous` call, and
    /// the mapping must not be used afterwards.
    pub unsafe fn unmap(ptr: *mut c_void, _len: usize) {
        // MEM_RELEASE frees the whole allocation and requires a size of 0.
        unsafe {
            VirtualFree(ptr, 0, MEM_RELEASE);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    const BUFFER_SIZE: usize = 16384;
    const ELEMENT_SIZE: usize = 2;

    #[test]
    fn test_anonymous_mmap() {
        let mmap = AnonymousMmap::new(BUFFER_SIZE).unwrap();
        assert_eq!(mmap.len, BUFFER_SIZE);
    }

    #[test]
    fn test_new_zero_len_is_err() {
        assert!(AnonymousMmap::new(0).is_err());
    }

    #[test]
    fn test_new_non_power_of_two_is_err() {
        assert!(AnonymousMmap::new(12_000).is_err());
    }

    #[test]
    fn test_new_smaller_than_page_is_err() {
        assert!(AnonymousMmap::new(8).is_err());
    }

    // Cloning would duplicate the owning pointer and unmap the region twice
    // (use-after-free for the surviving handle).
    static_assertions::assert_not_impl_any!(AnonymousMmap: Clone);

    #[derive(Clone)]
    struct TestMessageBus {
        buffer: std::sync::Arc<AnonymousMmapBuffer>,
    }

    struct AnonymousMmapBuffer {
        mmap: AnonymousMmap,
        write_head: std::sync::atomic::AtomicUsize,
        read_head: std::sync::atomic::AtomicUsize,
    }

    impl TestMessageBus {
        fn new(len: usize) -> Self {
            let mmap = AnonymousMmap::new(len).unwrap();
            let write_head = std::sync::atomic::AtomicUsize::new(0);
            let read_head = std::sync::atomic::AtomicUsize::new(0);
            Self {
                buffer: std::sync::Arc::new(AnonymousMmapBuffer {
                    mmap,
                    write_head,
                    read_head,
                }),
            }
        }

        fn read(&self, position: usize) -> Option<&[u8]> {
            let read_head_position = self
                .buffer
                .read_head
                .load(std::sync::atomic::Ordering::Acquire);
            assert!(
                position <= read_head_position,
                "position must be less than read_head"
            );
            if read_head_position == position {
                return None;
            }

            let wrapped_position = position % (self.buffer.mmap.len >> 1);

            let ptr = self.buffer.mmap.ptr as *const u8;
            let ptr = unsafe { ptr.add(wrapped_position) };

            Some(unsafe { std::slice::from_raw_parts(ptr, ELEMENT_SIZE) })
        }

        fn write(&self, data: &[u8]) {
            let position = self
                .buffer
                .write_head
                .fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);

            let wrapped_position = position % (self.buffer.mmap.len >> 1);

            let ptr = self.buffer.mmap.ptr as *mut u8;
            let ptr = unsafe { ptr.add(wrapped_position) };

            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            }

            let new_read_head = position + data.len();

            loop {
                match self.buffer.read_head.compare_exchange_weak(
                    position,
                    new_read_head,
                    std::sync::atomic::Ordering::Release,
                    std::sync::atomic::Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(_) => continue,
                }
            }
        }
    }

    #[test]
    #[should_panic]
    fn test_read_head_less_than_position() {
        let message_bus = TestMessageBus::new(BUFFER_SIZE);
        message_bus.read(1);
    }

    #[test]
    fn test_multi_threaded_anonymous_mmap() {
        let run_task = |mb: TestMessageBus, start: usize, step: usize, stop: usize| {
            let mut position = 0;
            let mut test = 0;

            loop {
                if let Some(data) = mb.read(position) {
                    let val = u16::from_ne_bytes([data[0], data[1]]);
                    assert_eq!(val, test);
                    position += ELEMENT_SIZE;
                    test += 1;
                    // exit if val is greater than stop
                    if val as usize > stop {
                        break;
                    }
                    // loop until val % step == 0
                    if (val as usize + step - start) % step == 0 {
                        mb.write((val + 1).to_ne_bytes().as_ref());
                    }
                }
            }
        };

        let len = BUFFER_SIZE;
        let mut handles = Vec::<std::thread::JoinHandle<()>>::new();
        let num_threads: usize = std::thread::available_parallelism()
            .expect("Failed to get number of available CPUs")
            .into();

        let stop = BUFFER_SIZE << 1;
        let message_bus = TestMessageBus::new(len);
        message_bus.write(0u16.to_ne_bytes().as_ref());

        for i in 0..num_threads {
            let mb = message_bus.clone();
            handles.push(std::thread::spawn(move || {
                run_task(mb, i, num_threads, stop)
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }
}

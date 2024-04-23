#[derive(Clone)]
pub struct AnonymousMmap {
    ptr: *mut libc::c_void,
    len: usize,
}

unsafe impl Sync for AnonymousMmap {}
unsafe impl Send for AnonymousMmap {}

impl AnonymousMmap {
    pub fn new(len: usize) -> Result<Self, std::io::Error> {
        assert_eq!(len & (len - 1), 0, "len must be a power of 2");
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) } as usize;
        assert!(
            len >= page_size,
            "len must at least page size: {len} >= {page_size}"
        );

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

        Ok(Self { ptr, len })
    }

    pub fn get_ptr(&self) -> *mut libc::c_void {
        self.ptr
    }
}

impl Drop for AnonymousMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    const BUFFER_SIZE: usize = 4096;
    const ELEMENT_SIZE: usize = 2;

    #[test]
    fn test_anonymous_mmap() {
        let mmap = AnonymousMmap::new(BUFFER_SIZE).unwrap();
        assert_eq!(mmap.len, BUFFER_SIZE);
    }

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

            Some(unsafe { std::slice::from_raw_parts(ptr as *const u8, ELEMENT_SIZE) })
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
                libc::memcpy(
                    ptr as *mut libc::c_void,
                    data.as_ptr() as *const libc::c_void,
                    data.len(),
                );
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

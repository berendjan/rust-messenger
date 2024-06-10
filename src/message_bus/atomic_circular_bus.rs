use crate::messenger;
use crate::mmap::anonymous_mmap;
use crate::traits;

/// A circular bus implementation that uses a shared memory buffer to store messages.
/// The buffer is shared between the writer and the reader.
/// This implementation returns immediately when there is no new message to read.
/// The writer and the reader are lock-free.
#[derive(Clone)]
pub struct CircularBus {
    buffer: std::sync::Arc<SharedBuffer>,
}

pub trait Config {
    const BUFFER_SIZE: usize;
}

struct SharedBuffer {
    mmap: anonymous_mmap::AnonymousMmap,
    write_head: std::sync::atomic::AtomicUsize,
    read_head: std::sync::atomic::AtomicUsize,
    wrap_size: usize,
}

impl CircularBus {
    pub fn new<C: Config>(_: &C) -> CircularBus {
        let mmap = anonymous_mmap::AnonymousMmap::new(C::BUFFER_SIZE).unwrap();
        let write_head = std::sync::atomic::AtomicUsize::new(0);
        let read_head = std::sync::atomic::AtomicUsize::new(0);
        let wrap_size = C::BUFFER_SIZE >> 1;
        Self {
            buffer: std::sync::Arc::new(SharedBuffer {
                mmap,
                write_head,
                read_head,
                wrap_size,
            }),
        }
    }
}

impl traits::Writer for CircularBus {
    fn write<M: traits::Message, H: traits::Handler, F: FnMut(&mut [u8])>(
        &self,
        size: usize,
        mut callback: F,
    ) {
        let aligned_size = messenger::align_to_usize(size);
        let len = messenger::ALIGNED_HEADER_SIZE + aligned_size;

        let position = self
            .buffer
            .write_head
            .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        let wrapped_pos = position % self.buffer.wrap_size;

        let ptr = self.buffer.mmap.get_ptr() as *mut u8;
        let ptr = unsafe { ptr.add(wrapped_pos) };

        unsafe {
            std::ptr::write_bytes(ptr, 0, len);
        }

        let header_ptr = ptr as *mut messenger::Header;
        unsafe {
            (*header_ptr).source = H::ID.into();
            (*header_ptr).message_id = M::ID.into();
            (*header_ptr).size = aligned_size as u16;
        }

        let msg_ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts_mut(msg_ptr, len) };
        callback(buffer);

        let new_read_head = position + len;
        loop {
            match self.buffer.read_head.compare_exchange_weak(
                position,
                new_read_head,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => break,
                _ => continue,
            }
        }
    }
}

impl traits::Reader for CircularBus {
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        let read_head_position = self
            .buffer
            .read_head
            .load(std::sync::atomic::Ordering::Acquire);

        if position >= read_head_position {
            return None;
        }
        let wrapped_position = position % self.buffer.wrap_size;

        let ptr = self.buffer.mmap.get_ptr() as *const u8;
        let ptr = unsafe { ptr.add(wrapped_position) };

        let header_ptr = ptr as *const messenger::Header;
        let header = unsafe { &*header_ptr };
        let len = header.size as usize;

        let ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts(ptr, len) };
        Some((header, buffer))
    }
}

impl traits::MessageBus for CircularBus {}

#[cfg(test)]
mod tests {

    use super::*;

    struct MsgA {
        data: [u16; 5],
    }

    impl traits::Message for MsgA {
        type Id = u16;
        const ID: u16 = 2;
    }

    #[cfg(not(feature = "zero_copy"))]
    impl traits::ExtendedMessage for MsgA {
        fn get_size(&self) -> usize {
            std::mem::size_of::<Self>()
        }
        fn write_into(&self, buffer: &mut [u8]) {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.data.as_ptr() as *const u8,
                    buffer.as_mut_ptr(),
                    10,
                )
            }
        }
    }

    #[cfg(feature = "zero_copy")]
    impl traits::ZeroCopyMessage for MsgA {}

    struct HandlerA {}

    struct Config {}

    impl super::Config for Config {
        const BUFFER_SIZE: usize = 4096;
    }

    impl traits::Handler for HandlerA {
        type Id = u16;
        const ID: u16 = 1;
        type Config = Config;
        fn new(_config: &Config) -> Self {
            Self {}
        }
    }

    #[cfg(not(feature = "zero_copy"))]
    #[test]
    fn test_circular_bus() {
        use crate::traits::Handler;
        use crate::traits::Message;
        use crate::traits::Reader;
        use crate::traits::Sender;

        let config = Config {};
        let bus = CircularBus::new(&config);
        let mut position: usize = 0;
        for i in 0..500 {
            let message = MsgA {
                data: [i, 1, 2, 3, 4],
            };
            HandlerA::send(&message, &bus);

            let (hdr, buffer) = bus.read(position).unwrap();

            assert_eq!(hdr.source, HandlerA::ID.into());
            assert_eq!(hdr.message_id, MsgA::ID.into());
            let expected_size = messenger::align_to_usize(std::mem::size_of::<MsgA>());
            assert_eq!(hdr.size, expected_size as u16);

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += messenger::ALIGNED_HEADER_SIZE + hdr.size as usize;
        }
    }

    #[cfg(feature = "zero_copy")]
    #[test]
    fn test_circular_bus() {
        use crate::traits::Handler;
        use crate::traits::Message;
        use crate::traits::Reader;
        use crate::traits::Sender;

        let config = Config {};
        let bus = CircularBus::new(&config);
        let mut position: usize = 0;
        for i in 0..500 {
            HandlerA::send::<MsgA, _, _>(&bus, |msg| {
                unsafe { (*msg).data = [i, 1, 2, 3, 4] };
            });

            let (hdr, buffer) = bus.read(position).unwrap();

            assert_eq!(hdr.source, HandlerA::ID.into());
            assert_eq!(hdr.message_id, MsgA::ID.into());
            let expected_size = messenger::align_to_usize(std::mem::size_of::<MsgA>());
            assert_eq!(hdr.size, expected_size as u16);

            let msg_ptr = buffer.as_ptr() as *const MsgA;
            let message = unsafe { &*msg_ptr };
            assert_eq!(message.data, [i, 1, 2, 3, 4]);

            position += messenger::ALIGNED_HEADER_SIZE + hdr.size as usize;
        }
    }
}

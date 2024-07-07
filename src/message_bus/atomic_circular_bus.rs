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
    fn get_buffer_size(&self) -> usize;
}

struct SharedBuffer {
    mmap: anonymous_mmap::AnonymousMmap,
    write_head: std::sync::atomic::AtomicUsize,
    read_head: std::sync::atomic::AtomicUsize,
    wrap_size: usize,
}

impl CircularBus {
    pub fn new<C: Config>(config: &C) -> CircularBus {
        let mmap = anonymous_mmap::AnonymousMmap::new(config.get_buffer_size()).unwrap();
        let write_head = std::sync::atomic::AtomicUsize::new(0);
        let read_head = std::sync::atomic::AtomicUsize::new(0);
        let wrap_size = config.get_buffer_size() >> 1;
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

impl traits::core::Writer for CircularBus {
    fn write<M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
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

        let hdr_ptr = ptr as *mut messenger::Header;
        unsafe {
            std::ptr::addr_of_mut!((*hdr_ptr).source).write(H::ID.into());
            std::ptr::addr_of_mut!((*hdr_ptr).message_id).write(M::ID.into());
            std::ptr::addr_of_mut!((*hdr_ptr).size).write(aligned_size as u16);
        }

        let msg_ptr = unsafe { ptr.add(messenger::ALIGNED_HEADER_SIZE) };
        let buffer = unsafe { std::slice::from_raw_parts_mut(msg_ptr, len) };
        callback(buffer);

        let new_read_head = position + len;
        loop {
            if let Ok(_) = self.buffer.read_head.compare_exchange_weak(
                position,
                new_read_head,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                break;
            }
        }
    }
}

impl traits::core::Reader for CircularBus {
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

impl traits::core::MessageBus for CircularBus {}

#[cfg(test)]
mod tests {

    use super::*;

    struct MsgA {
        data: [u16; 5],
    }

    impl traits::core::Message for MsgA {
        type Id = u16;
        const ID: u16 = 2;
    }

    impl traits::extended::ExtendedMessage for MsgA {
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

    impl traits::zero_copy::ZeroCopyMessage for MsgA {}

    struct HandlerA {}

    struct Config {}

    impl super::Config for Config {
        fn get_buffer_size(&self) -> usize {
            4096
        }
    }

    impl traits::core::Handler for HandlerA {
        type Id = u16;
        const ID: u16 = 1;
        type Config = Config;
        fn new(_config: &Config) -> Self {
            Self {}
        }
    }

    #[test]
    fn test_circular_bus() {
        use crate::traits::core::Handler;
        use crate::traits::core::Message;
        use crate::traits::core::Reader;
        use crate::traits::extended::Sender;

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

    #[test]
    fn test_zero_copy_circular_bus() {
        use crate::traits::core::Handler;
        use crate::traits::core::Message;
        use crate::traits::core::Reader;
        use crate::traits::zero_copy::Sender;

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

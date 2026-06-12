use crate::messenger;
use crate::traits;
use crate::traits::core::MessageBus;

struct Inner<MB: MessageBus> {
    cvar: std::sync::Condvar,
    // Only used to park readers; the bus synchronizes its own data.
    lock: std::sync::Mutex<()>,
    message_bus: MB,
    stop: std::sync::atomic::AtomicBool,
    // Number of readers parked (or about to park) on the condvar. Lets
    // writers skip the mutex + notify entirely when nobody is waiting.
    waiters: std::sync::atomic::AtomicUsize,
}

/// Wraps a [`MessageBus`] so that `read` blocks on a condition variable until
/// a message is available (or the bus is stopped) instead of returning `None`
/// immediately.
#[derive(Clone)]
pub struct CondvarBus<MB: MessageBus> {
    inner: std::sync::Arc<Inner<MB>>,
}

impl<M: MessageBus> CondvarBus<M> {
    pub fn new(message_bus: M) -> CondvarBus<M> {
        CondvarBus {
            inner: std::sync::Arc::new(Inner {
                cvar: std::sync::Condvar::new(),
                lock: std::sync::Mutex::new(()),
                message_bus,
                stop: std::sync::atomic::AtomicBool::new(false),
                waiters: std::sync::atomic::AtomicUsize::new(0),
            }),
        }
    }
}

impl<MB: MessageBus> traits::core::Writer for CondvarBus<MB> {
    fn write<M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        self.inner.message_bus.write::<M, H, F>(size, callback);
        // Dekker-style pairing with read(): the SeqCst fence orders the
        // message publication before the waiters load, and readers increment
        // waiters (SeqCst) before their final availability re-check. So if we
        // load 0 here, any reader that subsequently parks re-checked after
        // our publication and saw the message — no wakeup is lost.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        if self.inner.waiters.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            // Take the lock briefly so no reader can be between its re-check
            // and going to sleep when the notification fires.
            drop(self.inner.lock.lock().unwrap());
            self.inner.cvar.notify_all();
        }
    }
}

impl<MB: MessageBus> traits::core::Reader for CondvarBus<MB> {
    fn read(&self, position: usize) -> Option<(&messenger::Header, &[u8])> {
        loop {
            if let Some(result) = self.inner.message_bus.read(position) {
                return Some(result);
            }
            if self.inner.stop.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }

            let guard = self.inner.lock.lock().unwrap();
            // Register as a waiter BEFORE the final re-check (pairs with the
            // SeqCst fence in write); see the comment there.
            self.inner
                .waiters
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.inner.message_bus.read(position).is_none()
                && !self.inner.stop.load(std::sync::atomic::Ordering::Relaxed)
            {
                drop(self.inner.cvar.wait(guard).unwrap());
            }
            // Relaxed: a writer reading a stale non-zero count only performs
            // a harmless extra notify.
            self.inner
                .waiters
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl<MB: MessageBus> traits::core::MessageBus for CondvarBus<MB> {
    fn on_stop(&self) {
        self.inner
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _guard = self.inner.lock.lock().unwrap();
        self.inner.cvar.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_bus::atomic_circular_bus;
    use crate::traits::core::Reader;
    use crate::traits::extended::Sender;

    #[derive(Clone, Copy)]
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

    struct HandlerA {}
    impl traits::core::Handler for HandlerA {
        type Id = u16;
        const ID: u16 = 1;
    }

    struct Config {}
    impl atomic_circular_bus::Config for Config {
        fn get_buffer_size(&self) -> usize {
            16384
        }
    }

    #[test]
    fn test_blocked_reader_wakes_on_write() {
        let bus = CondvarBus::new(atomic_circular_bus::CircularBus::new(&Config {}));

        let (tx, rx) = std::sync::mpsc::channel();
        let bus2 = bus.clone();
        let reader = std::thread::spawn(move || {
            let (_, buffer) = bus2.read(0).expect("read returned None without stop");
            let first = u16::from_ne_bytes([buffer[0], buffer[1]]);
            tx.send(first).unwrap();
        });

        // Give the reader a chance to park before writing.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let message = MsgA {
            data: [42, 1, 2, 3, 4],
        };
        HandlerA::send(&message, &bus);

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("blocked reader was not woken by the write");
        assert_eq!(received, 42);
        reader.join().unwrap();
    }

    #[test]
    fn test_stop_unblocks_reader() {
        use crate::traits::core::MessageBus;

        let bus = CondvarBus::new(atomic_circular_bus::CircularBus::new(&Config {}));

        let (tx, rx) = std::sync::mpsc::channel();
        let bus2 = bus.clone();
        let reader = std::thread::spawn(move || {
            let result = bus2.read(0);
            tx.send(result.is_none()).unwrap();
        });

        std::thread::sleep(std::time::Duration::from_millis(20));
        bus.on_stop();

        let got_none = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("blocked reader was not unblocked by on_stop");
        assert!(got_none);
        reader.join().unwrap();
    }
}

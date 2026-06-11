use crate::messenger;
use crate::traits;
use crate::traits::core::MessageBus;

struct Inner<MB: MessageBus> {
    cvar: std::sync::Condvar,
    // Only used to park readers; the bus synchronizes its own data.
    lock: std::sync::Mutex<()>,
    message_bus: MB,
    stop: std::sync::atomic::AtomicBool,
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
        // Take the lock briefly so no reader can miss the notification
        // between its availability check and going to sleep.
        drop(self.inner.lock.lock().unwrap());
        self.inner.cvar.notify_all();
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
            if self.inner.message_bus.read(position).is_none()
                && !self.inner.stop.load(std::sync::atomic::Ordering::Relaxed)
            {
                drop(self.inner.cvar.wait(guard).unwrap());
            }
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

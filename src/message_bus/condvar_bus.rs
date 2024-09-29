use crate::messenger;
use crate::traits;
use crate::traits::core::MessageBus;

struct Inner<MB: MessageBus> {
    cvar: std::sync::Condvar,
    message_bus: std::sync::Mutex<MB>,
    stop: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct CondvarBus<MB: MessageBus> {
    inner: std::sync::Arc<Inner<MB>>,
}

impl<M: MessageBus> CondvarBus<M> {
    pub fn new(message_bus: M) -> CondvarBus<M> {
        CondvarBus {
            inner: std::sync::Arc::new(Inner {
                cvar: std::sync::Condvar::new(),
                message_bus: std::sync::Mutex::new(message_bus),
                stop: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }
}

impl<MB: MessageBus> traits::core::Writer for CondvarBus<MB> {
    fn write<'a, M: traits::core::Message, H: traits::core::Handler, F: FnOnce(&'a mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        let mb = self.inner.message_bus.lock().unwrap();
        mb.write::<M, H, F>(size, callback);
        self.inner.cvar.notify_all();
    }
}

impl<MB: MessageBus> traits::core::Reader for CondvarBus<MB> {
    fn read<'a>(&self, position: usize) -> Option<(&'a messenger::Header, &'a [u8])> {
        let mut mb = self.inner.message_bus.lock().unwrap();
        while mb.read(position).is_none()
            && !self.inner.stop.load(std::sync::atomic::Ordering::Relaxed)
        {
            mb = self.inner.cvar.wait(mb).unwrap();
        }
        mb.read(position)
    }
}

impl<MB: MessageBus> traits::core::MessageBus for CondvarBus<MB> {
    fn on_stop(&self) {
        self.inner
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.inner.cvar.notify_all();
    }
}

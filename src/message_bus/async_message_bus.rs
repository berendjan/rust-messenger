#[derive(Clone)]
pub struct AsyncMessageBusWrapper<MB: crate::traits::core::MessageBus> {
    message_bus: MB,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl<MB: crate::traits::core::MessageBus> AsyncMessageBusWrapper<MB> {
    pub fn new(message_bus: MB) -> Self {
        Self {
            message_bus,
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl<MB: crate::traits::core::MessageBus> crate::traits::core::Writer
    for AsyncMessageBusWrapper<MB>
{
    fn write<
        M: crate::traits::core::Message,
        H: crate::traits::core::Handler,
        F: FnOnce(&mut [u8]),
    >(
        &self,
        size: usize,
        callback: F,
    ) {
        self.message_bus.write::<M, H, F>(size, callback);
        self.notify.notify_waiters();
    }
}

impl<MB: crate::traits::core::MessageBus> crate::traits::async_traits::AsyncReader
    for AsyncMessageBusWrapper<MB>
{
    async fn async_read(&self, position: usize) -> (&crate::messenger::Header, &[u8]) {
        if let Some((header, buffer)) = self.message_bus.read(position) {
            return (header, buffer);
        }

        self.notify.notified().await;
        self.message_bus.read(position).unwrap()
    }
}

// So it can also be used by to read directly from the underlying MessageBus
impl<MB: crate::traits::core::MessageBus> std::ops::Deref for AsyncMessageBusWrapper<MB> {
    type Target = MB;

    fn deref(&self) -> &Self::Target {
        return &self.message_bus;
    }
}

impl<MB: crate::traits::core::MessageBus + Clone> crate::traits::async_traits::AsyncMessageBus
    for AsyncMessageBusWrapper<MB>
{
}

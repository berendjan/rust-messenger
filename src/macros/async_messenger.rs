/// make async message bus, with async read function.
///
/// on new message we can do the following:
/// option 1. start new task for each handler handling the message.
/// option 2. start new task worker for each handler.
/// option 3. start new task for each handler handling the message,
/// leave in handler syncing up to the caller by calling a synchronization
/// primitive in the handle call.
///
/// on option 1, a handler might handling 2 messages at the same time.
/// on option 2, we have deterministic behaviour as a handler can process
/// one message at a time, allowing for replayability at the cost of
/// reading the message bus many times. This cost increases greatly if many
/// components are involved.
/// on option 3, we have opt-in replayability.
///
/// handlers are no longer single threaded!
///

pub trait AsyncMessageBus: AsyncReader + Writer + Sync + Send {}

struct AsyncMessageBusWrapper<MB: MessageBus> {
    message_bus: MB,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl<MB: MessageBus> AsyncMessageBusWrapper<MB> {
    pub fn new<C: Config>(config: &C) -> AsyncMessageBusWrapper {
        let message_bus = MB::new(config);
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        Self {
            message_bus,
            notify,
        }
    }
}

impl<MB: MessageBus> traits::Writer for AsyncMessageBusWrapper<MB> {
    fn write<M: traits::Message, H: traits::Handler, F: FnOnce(&mut [u8])>(
        &self,
        size: usize,
        callback: F,
    ) {
        self.message_bus.write(size, callback);
        self.notify.notify_waiters();
    }
}

impl traits::AsyncReader for CircularBus {
    async fn read(&self, position: usize) -> (&messenger::Header, &[u8]) {
        if let Some((header, buffer)) = self.message_bus.read(position) {
            return (header, buffer);
        }

        self.notify.notified().await;
        self.message_bus.read(position).unwrap()
    }
}

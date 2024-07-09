//  make async message bus, with async read function.

//  on new message we can do the following:
//  option 1. start new task for each handler handling the message.
//  option 2. start new task worker for each handler.
//  option 3. start new task for each handler handling the message,
//  leave in handler syncing up to the caller by calling a synchronization
//  primitive in the handle call.

//  on option 1, a handler might handling 2 messages at the same time.
//  on option 2, we have deterministic behaviour as a handler can process
//  one message at a time, allowing for replayability at the cost of
//  reading the message bus many times. This cost increases greatly if many
//  components are involved.
//  on option 3, we have opt-in replayability.

//  handlers are no longer single threaded!

use rust_messenger::message_bus::async_message_bus::AsyncMessageBusWrapper;
use rust_messenger::message_bus::atomic_circular_bus::CircularBus;
use rust_messenger::messenger;
use rust_messenger::traits;
use rust_messenger::traits::async_traits::AsyncHandle;
use rust_messenger::traits::async_traits::AsyncHandler;
use rust_messenger::traits::async_traits::AsyncRouter;
use rust_messenger::traits::core::DeserializeFrom;
use rust_messenger::traits::core::Handler;
use rust_messenger::traits::core::Message;

pub struct Messenger<MB: traits::async_traits::AsyncMessageBus> {
    message_bus: MB,
    config: crate::config::Config,
    stop: std::sync::Arc<tokio::sync::Notify>,
}

impl Messenger<AsyncMessageBusWrapper<CircularBus>> {
    pub fn new(config: crate::config::Config) -> Messenger<AsyncMessageBusWrapper<CircularBus>> {
        let mb = CircularBus::new(&config);
        let amb = AsyncMessageBusWrapper::new(mb);
        Messenger {
            message_bus: amb,
            config,
            stop: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn run(&self) -> rust_messenger::messenger::JoinHandles {
        let mut handles = Vec::<std::thread::JoinHandle<()>>::new();

        let mb = self.message_bus.clone();
        let cf = self.config.clone();
        let st = self.stop.clone();
        handles.push(std::thread::spawn(|| AsyncWorker::run_task(mb, cf, st)));

        messenger::JoinHandles::new(handles)
    }

    pub fn stop(&self) {
        println!("Stopping Messenger, Goodbye!");
        self.stop.notify_waiters()
    }
}

struct AsyncWorker {
    position: usize,
    join_set: tokio::task::JoinSet<()>,
    handler_a: crate::handlers::HandlerA,
    handler_b: crate::handlers::HandlerB,
    handler_c: crate::handlers::HandlerC,
}

impl AsyncWorker {
    fn run_task<MB: traits::async_traits::AsyncMessageBus + 'static>(
        message_bus: MB,
        config: crate::config::Config,
        stop: std::sync::Arc<tokio::sync::Notify>,
    ) {
        let mut worker = AsyncWorker {
            position: 0,
            join_set: tokio::task::JoinSet::new(),
            handler_a: crate::handlers::HandlerA::new(&config),
            handler_b: crate::handlers::HandlerB::new(&config),
            handler_c: crate::handlers::HandlerC::new(&config),
        };

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config.threads)
            .enable_all()
            .build()
            .unwrap()
            .block_on(worker.run(message_bus, stop));
    }

    async fn run<MB: traits::async_traits::AsyncMessageBus + 'static>(
        &mut self,
        message_bus: MB,
        stop: std::sync::Arc<tokio::sync::Notify>,
    ) {
        self.handler_a.async_on_start(&message_bus).await;
        self.handler_b.async_on_start(&message_bus).await;
        self.handler_c.async_on_start(&message_bus).await;
        loop {
            tokio::select! {
                (header, buffer) = message_bus.async_read(self.position) => {
                    self.position += messenger::ALIGNED_HEADER_SIZE + buffer.len();
                    let mb = message_bus.clone();
                    self.route(&header, &buffer, mb).await;

                    self.handler_a.async_on_loop(&message_bus).await;
                    self.handler_b.async_on_loop(&message_bus).await;
                    self.handler_c.async_on_loop(&message_bus).await;
                },
                _ = stop.notified() => {
                    self.handler_a.async_on_stop().await;
                    self.handler_b.async_on_stop().await;
                    break
                },
            }
        }
        while let Some(_) = self.join_set.join_next().await {}
    }
}

impl traits::async_traits::AsyncRouter for AsyncWorker {
    #[inline]
    async fn route<W: traits::async_traits::AsyncMessageBus + 'static>(
        &mut self,
        header: &rust_messenger::messenger::Header,
        buffer: &[u8],
        writer: W,
    ) {
        match (header.source.into(), header.message_id.into()) {
            (crate::handlers::HandlerB::ID, crate::messages::MessageA::ID) => {
                let message =
                    std::sync::Arc::new(<crate::messages::MessageA>::deserialize_from(&buffer));

                let wrt = writer.clone();
                let msg: std::sync::Arc<crate::messages::MessageA> = message.clone();
                self.join_set
                    .spawn(async move { crate::handlers::HandlerA::handle(msg, wrt).await });
                let wrt = writer.clone();
                let msg = message.clone();
                self.join_set
                    .spawn(async move { crate::handlers::HandlerC::handle(msg, wrt).await });
            }

            (crate::handlers::HandlerA::ID, crate::messages::MessageB::ID) => {
                let message =
                    std::sync::Arc::new(<crate::messages::MessageB>::deserialize_from(&buffer));
                let wrt = writer.clone();
                let msg = message.clone();
                self.join_set
                    .spawn(async { crate::handlers::HandlerB::handle(msg, wrt).await });
            }

            _ => {}
        }
    }
}

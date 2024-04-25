/// This macro generates messenger routing and initialization code.
/// This includes: Messenger implementation, Worker structs,
/// Worker implementation and Router implementation for each worker.
///
/// example:
/// ``` ignore
/// Messenger! {
///     config::Config,
///     rust_messenger::message_bus::MemoryBus,
///     WorkerA:
///         handlers: [
///             handler_a: handlers::HandlerA,
///             handler_b: handlers::HandlerB,
///         ]
///         routes: [
///             handlers::HandlerA, messages::MessageB: [ handler_b ],
///             handlers::HandlerB, messages::MessageA: [ handler_a ],
///         ]
///     WorkerB:
///         handlers: [
///             handler_c: handlers::HandlerC,
///         ]
///         routes: [
///             handlers::HandlerB, messages::MessageA: [ handler_c ],
///         ]
/// }
/// ```
///
/// generates:
///
/// ```ignore
/// use rust_messenger::messenger;
/// use rust_messenger::traits;
/// use rust_messenger::traits::DeserializeMessage;
/// use rust_messenger::traits::Handle;
/// use rust_messenger::traits::Handler;
/// use rust_messenger::traits::Message;
/// use rust_messenger::traits::Router;
///
/// pub struct Messenger<MB: traits::MessageBus> {
///     message_bus: MB,
///     stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
///     config: config::Config,
/// }
///
/// impl rust_messenger::Messenger<MemoryBus> {
///     pub fn new(config: config::Config) -> rust_messenger::Messenger<MemoryBus> {
///         rust_messenger::Messenger {
///             message_bus: MemoryBus::new(),
///             stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
///             config,
///         }
///     }
///
///     pub fn run(&self) -> messenger::JoinHandles {
///         let mut handles = Vec::<std::thread::JoinHandle<()>>::new();
///
///         let mb = self.message_bus.clone();
///         let cf = self.config.clone();
///         let st = self.stop.clone();
///         handles.push(std::thread::spawn(|| WorkerA::run_task(mb, cf, st)));
///         let mb = self.message_bus.clone();
///         let cf = self.config.clone();
///         let st = self.stop.clone();
///         handles.push(std::thread::spawn(|| WorkerB::run_task(mb, cf, st)));
///
///         messenger::JoinHandlers::new(handles)
///     }
///
///     pub fn stop(&self) {
///         self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
///         println!("Stopping Messenger, Goodbye!");
///     }
/// }
///
/// struct WorkerA {
///     position: usize,
///     handler_a: handlers::HandlerA,
///     handler_b: handlers::HandlerB,
///     stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
/// }
/// struct WorkerB {
///     position: usize,
///     handler_c: handlers::HandlerC,
///     stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
/// }
///
/// impl WorkerA {
///     fn run_task<MB: traits::MessageBus>(mut message_bus: MB, config: config::Config, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
///         let mut worker = WorkerA {
///             position: 0,
///             handler_a: handlers::HandlerA::new(&config),
///             handler_b: handlers::HandlerB::new(&config),
///             stop,
///         };
///         worker.run(&mut message_bus)
///     }
///
///     fn run<MB: traits::MessageBus>(&mut self, message_bus: &mut MB) {
///         self.handler_a.on_start(message_bus);
///         self.handler_b.on_start(message_bus);
///         loop {
///             if let Some((header, buffer)) = message_bus.read(self.position) {
///                 self.position += messenger::ALIGNED_HEADER_SIZE + buffer.len();
///                 self.route(&header, &buffer, message_bus);
///             }
///
///             self.handler_a.on_loop(message_bus);
///             self.handler_b.on_loop(message_bus);
///
///             if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
///                 break;
///             }
///         }
///     }
/// }
///
/// impl WorkerB {
///     pub fn run_task<MB: traits::MessageBus>(mut message_bus: MB, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
///         let mut worker = WorkerB {
///             position: 0,
///             handler_c: handlers::HandlerC::new(),
///             stop,
///         };
///         worker.run(&mut message_bus)
///     }
///     pub fn run<MB: traits::MessageBus>(&mut self, message_bus: &mut MB) {
///         self.handler_c.on_start(message_bus);
///         loop {
///             if let Some((header, buffer)) = message_bus.read(self.position) {
///                 self.position += messenger::ALIGNED_HEADER_SIZE + buffer.len();
///                 self.route(&header, &buffer, message_bus);
///             }
///
///             self.handler_c.on_loop(message_bus);
///
///             if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
///                 break;
///             }
///         }
///     }
/// }
///
/// impl traits::Router for WorkerA {
///     #[inline]
///     fn route<W: traits::Writer>(&mut self, header: &rust_messenger::Header, buffer: &[u8], writer: &W) {
///         match (header.source.into(), header.message_id.into()) {
///             (handlers::HandlerB::ID, messages::MessageA::ID) => {
///                 let message = <$message>::deserialize_from(&buffer);
///                 self.handler_a.handle(&message, writer);
///             }
///
///             (handlers::HandlerA::ID, messages::MessageB::ID) => {
///                 let message = <$message>::deserialize_from(&buffer);
///                 self.handler_b.handle(&message, writer);
///             }
///             _ => {}
///         }
///     }
/// }
///
/// impl traits::Router for WorkerB {
///     #[inline]
///     fn route<W: traits::Writer>(&mut self, header: &rust_messenger::Header, buffer: &[u8], writer: &W) {
///         match (header.source.into(), header.message_id.into()) {
///             (handlers::HandlerB::ID, messages::MessageA::ID) => {
///                 let message = <$message>::deserialize_from(&buffer);
///                 self.handler_c.handle(&message, writer);
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
///
/// to run the Messenger you can do:
/// ``` ignore
/// pub fn main() {
///     let mut messenger = rust_messenger::new();
///     messenger.run();
/// }
/// ```
///
#[macro_export]
macro_rules! Messenger {
    (
        $config:ty,
        $message_bus:ty,
        $( $worker:ident:
        handlers: [ $( $handler_ident:ident: $handler_ty:ty $(,)? ),+ ]
        routes: [ $( $source:ty, $message:ty: [ $( $receiver:ident $(,)? ),+ ] ),+ $(,)? ]
        $(in_place)?
        $(from)?
    )+ ) => {
        use rust_messenger::messenger;
        use rust_messenger::traits;
        use rust_messenger::traits::DeserializeFrom;
        use rust_messenger::traits::Handle;
        use rust_messenger::traits::Handler;
        use rust_messenger::traits::Message;
        use rust_messenger::traits::Router;

        pub struct Messenger<MB: traits::MessageBus> {
            message_bus: MB,
            stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
            config: $config,
        }

        impl Messenger<$message_bus> {
            pub fn new(config: $config) -> Messenger<$message_bus> {
                Messenger {
                    message_bus: <$message_bus>::new(&config),
                    stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    config,
                }
            }

            pub fn run(&self) -> messenger::JoinHandles {
                let mut handles = Vec::<std::thread::JoinHandle<()>>::new();

                $(
                    let mb = self.message_bus.clone();
                    let cf = self.config.clone();
                    let st = self.stop.clone();
                    handles.push(std::thread::spawn(|| $worker::run_task(mb, cf, st)));
                )+

                messenger::JoinHandles::new(handles)
            }

            pub fn stop(&self) {
                self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                println!("Stopping Messenger, Goodbye!");
            }
        }

        $(
            struct $worker {
                position: usize,
                $($handler_ident: $handler_ty,)+
                stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
            }

            impl $worker {
                fn run_task<MB: traits::MessageBus>(mut message_bus: MB, config: $config, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
                    let mut worker = $worker {
                        position: 0,
                        $($handler_ident: <$handler_ty>::new(&config),)+
                        stop,
                    };
                    worker.run(&mut message_bus)
                }

                fn run<MB: traits::MessageBus>(&mut self, message_bus: &mut MB) {
                    $(
                        self.$handler_ident.on_start(message_bus);
                    )+
                    loop {
                        if let Some((header, buffer)) = message_bus.read(self.position) {
                            self.position += messenger::ALIGNED_HEADER_SIZE + buffer.len();
                            self.route(&header, &buffer, message_bus);
                        }

                        $(
                            self.$handler_ident.on_loop(message_bus);
                        )+

                        if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            }

            impl traits::Router for $worker {
                #[inline]
                fn route<W: traits::Writer>(&mut self, header: &messenger::Header, buffer: &[u8], writer: &W) {
                    match (header.source.into(), header.message_id.into()) {
                        $(
                            (<$source>::ID, <$message>::ID) => {
                                let message = <$message>::deserialize_from(&buffer);
                                $(
                                    self.$receiver.handle(&message, writer);
                                )+
                            }
                        )+
                        _ => {}
                    }
                }
            }
        )+
    };
}

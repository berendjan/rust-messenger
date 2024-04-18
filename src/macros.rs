/// This macro generates messenger routing and initialization code.
/// This includes: Messenger implementation, Worker structs,
/// Worker implementation and Router implementation for each worker.
///
/// example:
/// ``` ignore
/// Messenger! {
///     messenger::message_bus::MemoryBus,
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
/// use ::messenger::messenger;
/// use ::messenger::traits;
/// use ::messenger::traits::DeserializeMessage;
/// use ::messenger::traits::Handle;
/// use ::messenger::traits::Handler;
/// use ::messenger::traits::Message;
/// use ::messenger::traits::Router;
///
/// pub struct Messenger<MB: traits::MessageBus> {
///     pub message_bus: MB,
/// }
///
/// impl messenger::Messenger<MemoryBus> {
///     pub fn new() -> messenger::Messenger<MemoryBus> {
///         messenger::Messenger {
///             message_bus: MemoryBus::new(),
///         }
///     }
///
///     pub fn run(&self) {
///         let mut handles = Vec::<std::thread::JoinHandle<()>>::new();
///
///         let mb = self.message_bus.clone();
///         handles.push(std::thread::spawn(|| WorkerA::run_task(mb)));
///         let mb = self.message_bus.clone();
///         handles.push(std::thread::spawn(|| WorkerB::run_task(mb)));
///
///         for handle in handles {
///             handle.join().unwrap();
///         }
///     }
/// }
///
/// struct WorkerA {
///     position: usize,
///     handler_a: handlers::HandlerA,
///     handler_b: handlers::HandlerB,
/// }
/// struct WorkerB {
///     position: usize,
///     handler_c: handlers::HandlerC,
/// }
///
/// impl WorkerA {
///     fn run_task<MB: traits::MessageBus>(mut message_bus: MB) {
///         let mut worker = WorkerA {
///             position: 0,
///             handler_a: handlers::HandlerA::new(),
///             handler_b: handlers::HandlerB::new(),
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
///             self.handler_a.on_loop(message_bus);
///             self.handler_b.on_loop(message_bus);
///         }
///     }
/// }
///
/// impl WorkerB {
///     pub fn run_task<MB: traits::MessageBus>(mut message_bus: MB) {
///         let mut worker = WorkerB {
///             position: 0,
///             handler_c: handlers::HandlerC::new(),
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
///             self.handler_c.on_loop(message_bus);
///         }
///     }
/// }
///
/// impl traits::Router for WorkerA {
///     #[inline]
///     fn route<W: traits::Writer>(&mut self, header: &messenger::Header, buffer: &[u8], writer: &W) {
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
///     fn route<W: traits::Writer>(&mut self, header: &messenger::Header, buffer: &[u8], writer: &W) {
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
///     let mut messenger = Messenger::new();
///     messenger.run();
/// }
/// ```
///
#[macro_export]
macro_rules! Messenger {
    (
        $message_bus:ty,
        $( $worker:ident:
        handlers: [ $( $handler_ident:ident: $handler_ty:ty $(,)? ),+ ]
        routes: [ $( $source:ty, $message:ty: [ $( $receiver:ident $(,)? ),+ ] ),+ $(,)? ]
        $(in_place)?
        $(from)?
    )+ ) => {
        use ::messenger::messenger;
        use ::messenger::traits;
        use ::messenger::traits::DeserializeFrom;
        use ::messenger::traits::Handle;
        use ::messenger::traits::Handler;
        use ::messenger::traits::Message;
        use ::messenger::traits::Router;

        pub struct Messenger<MB: traits::MessageBus> {
            pub message_bus: MB,
        }

        impl Messenger<$message_bus> {
            pub fn new() -> Messenger<$message_bus> {
                Messenger {
                    message_bus: <$message_bus>::new(),
                }
            }

            pub fn run(&self) {
                let mut handles = Vec::<std::thread::JoinHandle<()>>::new();

                $(
                    let mb = self.message_bus.clone();
                    handles.push(std::thread::spawn(|| $worker::run_task(mb)));
                )+

                for handle in handles {
                    handle.join().unwrap();
                }
            }
        }

        $(
            struct $worker {
                position: usize,
                $($handler_ident: $handler_ty,)+
            }

            impl $worker {
                fn run_task<MB: traits::MessageBus>(mut message_bus: MB) {
                    let mut worker = $worker {
                        position: 0,
                        $($handler_ident: <$handler_ty>::new(),)+
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

//! The Messenger! macro must expand without type-inference ambiguity even
//! when the downstream crate graph adds extra `PartialEq<_> for u16` impls
//! (serde_json and tiny_http both do). This file reproduces that situation
//! hermetically: with a bare `.into()` in the generated route comparison,
//! the `impl PartialEq<Ambiguous> for u16` below makes this fail to compile
//! with E0283.

// No `use rust_messenger::traits;` here: the Messenger! macro expands its own
// imports (traits, Handle, Handler, Message, Router) into this module.
use rust_messenger::traits::extended::Sender;

/// The poison: a second `PartialEq` impl on u16, like serde_json's
/// `PartialEq<Value> for u16`.
pub struct Ambiguous;

impl PartialEq<Ambiguous> for u16 {
    fn eq(&self, _other: &Ambiguous) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct Config {
    pub stop_after: u16,
}

rust_messenger::messenger_id_enum!(
    HandlerId {
        Ping = 1,
        Pong = 2,
    }
);

rust_messenger::messenger_id_enum!(
    MessageId {
        Ball = 1,
    }
);

#[derive(Debug)]
pub struct Ball {
    pub bounces: u16,
}

impl traits::core::Message for Ball {
    type Id = MessageId;
    const ID: MessageId = MessageId::Ball;
}

impl Ball {
    pub fn deserialize_from(buffer: &[u8]) -> Self {
        Ball { bounces: u16::from_ne_bytes([buffer[0], buffer[1]]) }
    }
}

impl traits::extended::ExtendedMessage for Ball {
    fn get_size(&self) -> usize {
        2
    }
    fn write_into(&self, buffer: &mut [u8]) {
        buffer[..2].copy_from_slice(&self.bounces.to_ne_bytes());
    }
}

pub struct Ping {
    stop_after: u16,
}

impl Ping {
    pub fn new<W: traits::core::Writer>(config: &Config, _writer: &W) -> Self {
        Ping { stop_after: config.stop_after }
    }
}

impl traits::core::Handler for Ping {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::Ping;

    fn on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        Self::send(&Ball { bounces: 0 }, writer);
    }
}

impl traits::core::Handle<Ball> for Ping {
    fn handle<W: traits::core::Writer>(&mut self, message: &Ball, writer: &W) {
        if message.bounces < self.stop_after {
            Self::send(&Ball { bounces: message.bounces + 1 }, writer);
        }
    }
}

pub struct Pong;

impl Pong {
    pub fn new<W: traits::core::Writer>(_config: &Config, _writer: &W) -> Self {
        Pong
    }
}

impl traits::core::Handler for Pong {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::Pong;
}

impl traits::core::Handle<Ball> for Pong {
    fn handle<W: traits::core::Writer>(&mut self, message: &Ball, writer: &W) {
        Self::send(&Ball { bounces: message.bounces }, writer);
    }
}

rust_messenger::Messenger! {
    Config,
    WorkerA:
        handlers: [
            ping: Ping,
        ]
        routes: [
            Pong, Ball: [ ping ],
        ]
    WorkerB:
        handlers: [
            pong: Pong,
        ]
        routes: [
            Ping, Ball: [ pong ],
        ]
}

#[test]
fn messenger_compiles_and_runs_despite_extra_partial_eq_impls() {
    use rust_messenger::traits::core::Reader;

    struct BusConfig;
    impl rust_messenger::message_bus::atomic_circular_bus::Config for BusConfig {
        fn get_buffer_size(&self) -> usize {
            1 << 20
        }
    }

    let circular =
        rust_messenger::message_bus::atomic_circular_bus::CircularBus::new(&BusConfig);
    let bus = rust_messenger::message_bus::condvar_bus::CondvarBus::new(circular.clone());
    let messenger = Messenger::new(bus);
    let handles = messenger.run(&Config { stop_after: 4 });

    // Watch the raw bus until the ping handler stops volleying.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut position = 0;
    let mut last_bounces = 0;
    while std::time::Instant::now() < deadline && last_bounces < 4 {
        while let Some((header, buffer)) = circular.read(position) {
            position += rust_messenger::messenger::ALIGNED_HEADER_SIZE + header.size as usize;
            if header.message_id == u16::from(MessageId::Ball) {
                last_bounces = last_bounces.max(Ball::deserialize_from(buffer).bounces);
            }
        }
        std::thread::yield_now();
    }

    messenger.stop();
    handles.join();
    assert_eq!(last_bounces, 4, "ping-pong volley did not complete");
}

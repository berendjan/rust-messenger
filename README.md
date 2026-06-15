# Messenger

Library for creating software with a nano-services architecture. Each service (implementing `traits::Handler`) can receive and send messages but is unaware of any other service.
The routing table is what links all the services together.

## Usage example

Please see `examples/serde_bincode`.

To run:

```bash
cargo run --example serde_bincode
cargo run --example zero_copy
cargo run --example connect_rust  # protobuf messages via the buffa runtime
```

## Dev Quickstart

```bash
rustup toolchain install nightly
rustup +nightly component add miri
rustup override set nightly

cargo miri test

cargo miri run --example serde_bincode
cargo miri run --example zero_copy
```

This library implements a nano-services model where _handlers_ are tiny services.

## Overview

The library consists of 3 main parts:

1. A message bus that worker threads can read messages from and write message to.
2. A routing table that specifies for each worker where message with a source and message id combination is supposed to be routed to.
3. Handlers that are the nano-services that can only receive messages and send messages back.

The source code consists of 5 main parts, where the MessageBus can be changes depending on the needs of the user.

1. `messenger.rs` contains the header object.
2. `macros/` generates the routing logic and the worker objects.
3. `traits/` specifies all the traits that are implementable.
4. `message_bus/` contains possible implementations of the message bus required for concurrently sending/receiving data, uses mmap wrappers.
5. `mmap/` contains mmap wrappers for the message bus implementations.

The user is left to implement _handlers_ services which implement the `Handler` and `Handle` traits and the _messages_ that will be sent between _handlers_.

## Features

The library has 3 operating modes:

1. default
2. zero_copy
3. async

### default mode

This is used for serializing with other libraries such as `Prost` or `Serde`. Implement `traits::core::Message` (the source/id metadata) and `traits::extended::ExtendedMessage` (size + serialize) for each message; deserialization is a plain inherent method the routing macro calls. Sending then uses the `traits::extended::Sender` blanket impl.

Example trait `serde` (bincode 2.x) implementation macro from `examples/serde_bincode`:

```rust
use rust_messenger::traits;

rust_messenger::messenger_id_enum!(
    MessageId {
        MessageA = 0,
        MessageB = 1,
    }
);

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::core::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

        impl $type {
            pub fn deserialize_from(buffer: &[u8]) -> Self {
                bincode::serde::borrow_decode_from_slice(buffer, bincode::config::standard())
                    .unwrap()
                    .0
            }
        }

        impl traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                bincode::serde::encode_to_vec(self, bincode::config::standard())
                    .unwrap()
                    .len()
            }

            fn write_into(&self, buffer: &mut [u8]) {
                bincode::serde::encode_into_slice(self, buffer, bincode::config::standard())
                    .unwrap();
            }
        }
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);
```

Equivalent implementation for [`connect-rust`](https://github.com/anthropics/connect-rust) message types, which serialize via the [`buffa`](https://docs.rs/buffa) protobuf runtime (`buffa::Message`: `encoded_len`, `encode`, `decode_from_slice`). A complete, runnable version is in `examples/connect_rust` (`cargo run --example connect_rust`):

```rust
use buffa::Message; // brings encode_to_vec / decode_from_slice into scope

rust_messenger::messenger_id_enum!(
    MessageId {
        GetAccountRequest = 1,
        GetAccountResponse = 2,
    }
);

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl rust_messenger::traits::core::Message for $type {
            type Id = MessageId;
            const ID: Self::Id = $id;
        }

        impl rust_messenger::traits::extended::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                self.encode_to_vec().len()
            }

            fn write_into(&self, buffer: &mut [u8]) {
                let bytes = self.encode_to_vec();
                buffer[..bytes.len()].copy_from_slice(&bytes);
            }
        }

        impl $type {
            pub fn deserialize_from(buffer: &[u8]) -> Self {
                <$type>::decode_from_slice(buffer).unwrap()
            }
        }
    };
}

impl_message_traits!(account::GetAccountRequest, MessageId::GetAccountRequest);
impl_message_traits!(account::GetAccountResponse, MessageId::GetAccountResponse);
```

`get_size`/`write_into` encode twice here (once to size, once to copy); if your `buffa` version exposes an encoded-length and an encode-into-slice method, use those to encode once. `buffa` also generates zero-copy view types (`FooView<'a>` via `decode_view`, borrowing `string`/`bytes` straight from the buffer) — a natural fit for reading messages straight off the bus without allocating.

### zero_copy mode

This can be used when all messages are reinterpretable from a slice of bytes (by casting `*const u8` to `&Message`); each message type implements `traits::zero_copy::ZeroCopyMessage`. The trait requires `Copy + 'static` and rejects over-aligned types at compile time, but it **cannot** verify the bytes are a valid instance — see [Safety & known issues](#safety--known-issues). If you persist messages in a file-backed mmap, also make each type `#[repr(C)]` for a deterministic layout across builds.

Sending uses the `traits::zero_copy::Sender` blanket impl, whose callback receives a `*mut Message` into the bus buffer. The payload is not pre-zeroed, so write every field; `std::ptr::addr_of_mut!((*ptr).field).write(value)` avoids forming a reference to uninitialized memory:

```rust
use rust_messenger::traits;
use rust_messenger::traits::zero_copy::Sender;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MessageB {
    pub other_val: u16,
}

impl traits::core::Message for MessageB {
    type Id = MessageId;
    const ID: MessageId = MessageId::MessageB;
}
impl traits::zero_copy::ZeroCopyMessage for MessageB {}

impl MessageB {
    // The reader side: reinterpret the buffer bytes in place.
    pub fn deserialize_from(buffer: &[u8]) -> &Self {
        assert!(buffer.len() >= std::mem::size_of::<Self>());
        let ptr = buffer.as_ptr() as *const Self;
        assert!(ptr.is_aligned());
        unsafe { &*ptr }
    }
}

// The writer side, inside a handler:
Self::send::<MessageB, _, _>(writer, |msg| unsafe {
    std::ptr::addr_of_mut!((*msg).other_val).write(0)
});
```

## Safety & known issues

This is a lock-free, shared-memory library built on `unsafe`. Most of the bus
internals (raw pointers, `mmap`, atomic publication) are encapsulated, but a
few obligations and limitations are unavoidably the user's responsibility.
Read these before using it for anything load-bearing.

### Caller obligations (violating these is undefined behaviour)

- **`ZeroCopyMessage` types must be valid from arbitrary bytes.** A reader
  reinterprets buffer bytes as `&Message`, so the type must have no invalid
  bit patterns (no `bool`, `enum`, `char`, `NonZero*`, references, `Box`,
  `Vec`, …) and must own no heap data. The trait requires `Copy + 'static`
  and rejects over-aligned types at compile time, but it **cannot** check
  validity-from-bytes — that is on you. For file-backed/replayed buses also
  make every message `#[repr(C)]` so the layout is stable across builds, and
  write every byte (padding included), since padding is not guaranteed zero.
- **`ExtendedMessage::write_into` must write exactly `get_size()` bytes** and
  `get_size()` must not exceed the bus capacity. A mismatch panics inside the
  write callback (see below).

### Design limitations to be aware of

- **A panicking write callback is not recoverable cleanly.** Publication is a
  per-slot commit stamp set as the writer's last step, so a panic leaves an
  uncommitted hole: other writers are unaffected, but in-order readers (and
  the `ExtendingBus` reopen scan) stop at that position. Keep callbacks
  infallible, or build with `panic = "abort"`. Note the `ExtendedMessage`
  senders unwrap serialization errors, so a serializer that fails or
  disagrees with `get_size()` will panic here.
- **`CircularBus` readers that fall behind lose data.** Writers never wait for
  readers; a reader more than half the buffer behind has had its slot
  overwritten. `read` detects this and **panics** rather than returning torn
  data — but a reference already handed out can still be overwritten in place
  if a writer laps the ring while a handler holds it. Size the buffer for the
  worst-case reader lag, and copy data out if you hold it across long work.
  `ExtendingBus` does not have this failure mode (it never overwrites), but
  trades it for unbounded growth up to a fixed reservation, then panics.
- **Slot commit detection is probabilistic in the worst case.** A reader
  confirms a slot by matching a 64-bit position stamp. For honest payloads
  the chance of stale bytes forging a valid stamp is ~2⁻⁶⁴; this assumes
  payloads do not embed bus positions as `u64` at slot offsets, so **do not
  echo bus positions into message payloads**.
- **Routing matches on raw `(source, message_id)` `u16` pairs.** Two distinct
  `messenger_id_enum`s that map different variants to the same `u16` are
  indistinguishable to a router and will deserialize a message as the wrong
  type. Keep message ids globally unique. Unknown ids are ignored (they do not
  panic), but `messenger_id_enum`'s `from_u16` panics on unknown input — use
  the generated `TryFrom<u16>` for ids coming off the wire or out of a file.
- **Platform support.** `AnonymousMmap` / `CircularBus` are Unix + Windows;
  `ExtendingMmap` / `ExtendingBus` are Linux-only (they rely on `fallocate`
  and `MAP_FIXED`).

## Todo

- [x] Linux Growable Mmap Wrapper (`mmap::linux::ExtendingMmap`)
- [ ] Macos Growable Mmap Wrapper
- [ ] Windows Growable Mmap Wrapper (`VirtualAlloc2` + `MapViewOfFile3`)
- [x] Persistent (File Backed) Message Bus (`message_bus::ExtendingBus`)
- [x] Add Replay Functionality for Persistent (File Backed) Message Bus (`ExtendingBus` replays from position 0 and resumes appending on reopen)
- [x] Condvar Message Bus, that blocks if there are no new messages to be read, write should notify_all
- [x] remove zero copy feature
- [x] Linux Anonymous Mmap Wrapper
- [x] Cross-platform Anonymous Mmap Wrapper (Windows `VirtualAlloc`)
- [x] Lock-free per-slot commit publication (writers publish independently, no `read_head` chain)
- [x] Stop functionality
- [x] Added user configuration input
- [x] `Messenger::run()` returns a `Vec<JoinHandler>` wrapper class that will join the handles in the drop implementation.
- [ ] Drain-on-stop: process remaining buffered messages before workers exit (today `stop()` ends the loops with messages possibly still queued)
- [ ] Type-safe routing that prevents message-id collisions across separate id enums
- [ ] Enforce/verify the `ExtendedMessage` size contract instead of panicking in the write callback

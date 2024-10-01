# Messenger

Library for creating software with a nano-services architecture. Each service (implementing `traits::Handler`) can receive and send messages but is unaware of any other service.
The routing table is what links all the services together.

## Usage example

Please see `examples/serde_bincode`.

To run:

```bash
cargo run --example serde_bincode
cargo run --example zero_copy
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

This is used for serializing with other libraries such as `Prost` or `Serde`. It is recommended to implement `traits::ExtendedMessage` for each message.

Example trait `serde` implementation macro from `examples/serde_bincode`:

```rust
macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl traits::Message for $type {
            type Id = MessageId;
            const ID: MessageId = $id;
        }

        impl traits::DeserializeFrom for $type {
            fn deserialize_from(buffer: &[u8]) -> Self {
                bincode::deserialize(buffer).unwrap()
            }
        }

        impl traits::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                bincode::serialized_size(self).unwrap() as usize
            }

            fn write_into(&self, buffer: &mut [u8]) {
                bincode::serialize_into(buffer, self).unwrap();
            }
        }
    };
}

impl_message_traits!(MessageA, MessageId::MessageA);
impl_message_traits!(MessageB, MessageId::MessageB);
```

example prost trait implementation macro

```rust
rust_messenger::messenger_id_enum!(
    MessageId {
        GetAccountRequest = 1,
        GetAccountResponse = 2,
    }
);

macro_rules! impl_message_traits {
    ($type:ty, $id:expr) => {
        impl rust_messenger::traits::Message for $type {
            type Id = MessageId;
            const ID: Self::Id = $id;
        }

        impl rust_messenger::traits::ExtendedMessage for $type {
            fn get_size(&self) -> usize {
                self.encoded_len()
            }

            fn write_into(&self, mut buffer: &mut [u8]) {
                self.encode_raw(&mut buffer);
            }
        }

        impl rust_messenger::traits::DeserializeFrom for $type {
            fn deserialize_from(buffer: &[u8]) -> Self {
                Self::decode(buffer.to_vec().as_slice()).unwrap()
            }
        }
    };
}

impl_message_traits!(account::GetAccountRequest, MessageId::GetAccountRequest);
impl_message_traits!(account::GetAccountResponse, MessageId::GetAccountResponse);

```

### zero_copy mode

This can be used when all messages are reinterpretable from a slice of bytes (by casting `*mut u8` to `&Message`) and each message type needs to implement `traits::ZeroCopyMessage`.
Note that if you choose the persist the messages in a file-backed mmap, you should ensure that each type is `#[repr(C)]` for deterministic memory layout over consecutive builds.
For writing to the message bus you provide a callback with a `ptr` to a zero'd buffer. To prevent UB you may use `std::ptr::addr_of_mut((*ptr).field).write(value);`, see examples.

## Todo

- [ ] Linux Growable Mmap Wrapper
- [ ] Macos Growable Mmap Wrapper
- [ ] Persistent (File Backed) Message Bus
- [x] Condvar Message Bus, that blocks if there are no new messages to be read, write should notify_all
- [ ] Add Replay Functionality for Persistant (File Backed) Message Bus
- [x] remove zero copy feature
- [x] Linux Anonymous Mmap Wrapper
- [x] Stop functionality
- [x] Added user configuration input
- [x] `Messenger::run()` returns a `Vec<JoinHandler>` wrapper class that will join the handles in the drop implementation.

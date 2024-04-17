# Messenger

## Quickstart

```bash
rustup toolchain install nightly
rustup override set nightly
rustup +nightly component add miri
cargo miri run --bin two_workers_example
```

This library implements a nano-services model where *handlers* are tiny services.

## Overview

The library consists of 3 main parts:

1. A message bus that worker threads can read messages from and write message to.
2. A routing table that specifies for each worker where message with a source and message id combination is supposed to be routed to.
3. Handlers that are the nano-services that can only receive messages and send messages back.

Any implementation consists of 4 main parts, where the MessageBus can be changes depending on the needs of the user.

1. `messenger.rs` contains the root object.
2. `macro.rs` generates the routing logic and the worker objects.
3. `traits.rs` specifies all the traits that are implemented.
4. `message_bus/` contains possible implementations of the message bus required for concurrently sending/receiving data.
5. `mmap/` contains mmap wrappers for the message bus implementations.

The user is left to implement *handlers* services which implement the `Handler` and `Handle` traits and the *messages* that will be sent between *handlers*.

## Features

The library has 2 operating modes (as features):
1. default
2. zero_copy

### default mode
This is used for serializing with other libraries such as `Prost` or `Serde`. Recommended to implement `traits::ExtendedMessage` for each message.

### zero_copy mode
This can be used when all messages are reinterpretable from a slice of bytes (by casting `*mut u8` to `&Message`) and each message type needs to implement `traits::ZeroCopyMessage`.
Note that if you choose the persist the messages in a file-backed mmap, you should ensure that each type is `#[repr(C)]` for deterministic memory layout.

## Todo

- [ ] Linux Anonymous Mmap Wrapper
- [ ] Linux Growable Mmap Wrapper
- [ ] Macos Growable Mmap Wrapper
- [ ] Persistent (File Backed) Message Bus
- [ ] Add Replay Functionality for Persistant (File Backed) Message Bus

//! Quick hot-path microbenchmark: single-threaded write+read round trip.
//! Run with: cargo run --release --example bench

use rust_messenger::message_bus::atomic_circular_bus::{CircularBus, Config};
use rust_messenger::traits;
use rust_messenger::traits::core::Reader;
use rust_messenger::traits::zero_copy::Sender;

struct Cfg;
impl Config for Cfg {
    fn get_buffer_size(&self) -> usize {
        1 << 20
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Msg {
    data: [u64; 4],
}
impl traits::core::Message for Msg {
    type Id = u16;
    const ID: u16 = 1;
}
impl traits::zero_copy::ZeroCopyMessage for Msg {}

struct Bench;
impl traits::core::Handler for Bench {
    type Id = u16;
    const ID: u16 = 7;
}

fn main() {
    let bus = CircularBus::new(&Cfg);
    let n: usize = 20_000_000;
    let mut position = 0usize;

    // Warmup lap so the buffer is paged in.
    for i in 0..100_000u64 {
        Bench::send::<Msg, _, _>(&bus, |msg| unsafe { (*msg).data = [i, i, i, i] });
        let (header, _) = bus.read(position).unwrap();
        position += header.slot_len();
    }

    let start = std::time::Instant::now();
    for i in 0..n as u64 {
        Bench::send::<Msg, _, _>(&bus, |msg| unsafe { (*msg).data = [i, i, i, i] });
        let (header, buffer) = bus.read(position).unwrap();
        std::hint::black_box(buffer.as_ptr());
        position += header.slot_len();
    }
    let elapsed = start.elapsed();
    println!(
        "zero-copy write+read: {:.2} ns/op ({} ops in {:?})",
        elapsed.as_nanos() as f64 / n as f64,
        n,
        elapsed
    );

    // Read-only polling of an empty position (worker idle loop cost).
    let start = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(bus.read(std::hint::black_box(position)).is_none());
    }
    let elapsed = start.elapsed();
    println!(
        "empty poll:           {:.2} ns/op",
        elapsed.as_nanos() as f64 / n as f64
    );
}

//! A lock-free queue of timestamped input messages (MIDI, and the machine's keys, encoders and pot) from one producing thread (the UI, a MIDI port) to one consuming thread (the audio callback).
//!
//! The audio thread must never wait for a lock the UI thread holds (a stall is a dropout), and a note should land on its own frame, not at the start of the
//! next buffer. The producer stamps each message with the host's monotonic microsecond clock and pushes it; the consumer (`Instrument::render_events`)
//! takes the messages that fall in the buffer it is about to render and applies each at the right frame. Nothing here allocates after `new`, locks or blocks.
//!
//! **Single producer, single consumer**: that is the contract, not a check. Two producers need two queues (or a lock of their own on their side).
//! A full queue refuses a push and counts it (`dropped`); the oldest message is never overwritten, so what was queued is played in order.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// A message longer than this is not queued (note, controller, pitch bend and program change fit; a system-exclusive does not, and the instrument ignores it anyway).
pub const MAX_MSG: usize = 3;

/// What a message is. MIDI carries 1 to 3 bytes; the others carry their numbers in `bytes[0]` and `val`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Midi,
    /// `bytes[0]` the matrix id, `bytes[1]` 1 for down
    Key,
    /// `bytes[0]` the encoder, `val` the clicks
    Encoder,
    /// `val` the pot, 0 to 1023
    Pot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub ts_us: u64,
    pub kind: Kind,
    pub len: u8,
    pub bytes: [u8; MAX_MSG],
    pub val: i32,
}

const EMPTY: Message = Message { ts_us: 0, kind: Kind::Midi, len: 0, bytes: [0; MAX_MSG], val: 0 };

pub struct EventQueue {
    slots: Box<[UnsafeCell<Message>]>,
    mask: usize,
    /// the next slot the producer writes (only it stores to it) and the next the consumer reads (only it stores to it)
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

// The slots are touched by one producer and one consumer, ordered by `head` and `tail` (release on the writer, acquire on the reader).
unsafe impl Sync for EventQueue {}
unsafe impl Send for EventQueue {}

impl EventQueue {
    /// A queue of at least `capacity` messages (rounded up to a power of two, 16 to 65536).
    pub fn new(capacity: usize) -> EventQueue {
        let n = capacity.clamp(16, 65_536).next_power_of_two();
        EventQueue { slots: (0..n).map(|_| UnsafeCell::new(EMPTY)).collect(), mask: n - 1, head: AtomicUsize::new(0), tail: AtomicUsize::new(0), dropped: AtomicU64::new(0) }
    }

    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Messages waiting (exact from the consumer's side, a moment old from any other).
    pub fn len(&self) -> usize {
        self.head.load(Ordering::Acquire).wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pushes MIDI from the producer thread: false (and a count) when the queue is full or the message is empty or longer than `MAX_MSG`.
    pub fn push(&self, ts_us: u64, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > MAX_MSG {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let mut m = EMPTY;
        m.ts_us = ts_us;
        m.len = bytes.len() as u8;
        m.bytes[..bytes.len()].copy_from_slice(bytes);
        self.push_message(m)
    }

    /// A key of the machine's matrix (0 to 40) goes down or up.
    pub fn push_key(&self, ts_us: u64, id: u8, down: bool) -> bool {
        self.push_message(Message { ts_us, kind: Kind::Key, len: 2, bytes: [id, u8::from(down), 0], val: 0 })
    }

    /// An encoder (0 to 6) turns by `clicks`.
    pub fn push_encoder(&self, ts_us: u64, enc: u8, clicks: i32) -> bool {
        self.push_message(Message { ts_us, kind: Kind::Encoder, len: 1, bytes: [enc, 0, 0], val: clicks })
    }

    /// The pot moves to `value` (0 to 1023).
    pub fn push_pot(&self, ts_us: u64, value: i32) -> bool {
        self.push_message(Message { ts_us, kind: Kind::Pot, len: 0, bytes: [0; MAX_MSG], val: value })
    }

    fn push_message(&self, m: Message) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        if head.wrapping_sub(self.tail.load(Ordering::Acquire)) > self.mask {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // SAFETY: the slot at `head` is not readable by the consumer until `head` moves past it, and the producer is the only writer of it.
        unsafe { *self.slots[head & self.mask].get() = m };
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// From the consumer thread: the oldest message without taking it.
    pub fn peek(&self) -> Option<Message> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: the producer finished writing this slot before it moved `head` past it, and does not touch it again until `tail` moves past it.
        Some(unsafe { *self.slots[tail & self.mask].get() })
    }

    /// From the consumer thread: takes the oldest message.
    pub fn pop(&self) -> Option<Message> {
        let m = self.peek()?;
        self.tail.store(self.tail.load(Ordering::Relaxed).wrapping_add(1), Ordering::Release);
        Some(m)
    }

    /// Pushes refused so far.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Maps message timestamps to frames of the buffer being rendered. The window start follows the sample clock (each buffer begins where the last ended) and is
/// pulled back to the host clock only when the two drift apart by more than a buffer, so the jitter of when the callback happens to run does not move notes.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    start_us: Option<f64>,
}

impl Clock {
    pub fn new() -> Clock {
        Clock { start_us: None }
    }

    /// The window of the buffer of `frames` at `rate` that is being rendered at host time `now_us`: the messages stamped from the end of the last buffer to `now_us`
    /// are played in it (so a note pressed just before the callback sounds with about one buffer of delay, steady, instead of a random fraction of two).
    /// Returns `(start_us, frames_per_us)`.
    pub fn window(&mut self, now_us: u64, frames: usize, rate: f64) -> (f64, f64) {
        let dur = frames as f64 * 1e6 / rate;
        let ideal = now_us as f64 - dur;
        let start = match self.start_us {
            Some(s) if (s - ideal).abs() <= dur => s,
            _ => ideal,
        };
        self.start_us = Some(start + dur);
        (start, rate / 1e6)
    }

    pub fn reset(&mut self) {
        self.start_us = None;
    }
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn fifo_order_capacity_and_refusals() {
        let q = EventQueue::new(20);
        assert_eq!(q.capacity(), 32);
        assert!(q.peek().is_none() && q.pop().is_none() && q.is_empty());
        for i in 0..32u8 {
            assert!(q.push(i as u64, &[0x90, i, 100]));
        }
        assert!(!q.push(99, &[0x90, 1, 1]), "full: refused, nothing overwritten");
        assert_eq!(q.dropped(), 1);
        assert_eq!(q.len(), 32);
        for i in 0..32u8 {
            let m = q.pop().unwrap();
            assert_eq!((m.ts_us, m.len, m.bytes, m.kind), (i as u64, 3, [0x90, i, 100], Kind::Midi));
        }
        assert!(q.pop().is_none());
        assert!(q.push(1, &[0xC0, 5]) && q.pop().unwrap().len == 2);
        assert!(!q.push(1, &[]) && !q.push(1, &[1, 2, 3, 4]), "empty and long messages are refused");
        assert_eq!(q.dropped(), 3);
    }

    #[test]
    fn keys_encoders_and_the_pot_keep_their_numbers_and_their_order() {
        let q = EventQueue::new(16);
        assert!(q.push_key(1, 40, true) && q.push_encoder(2, 6, -3) && q.push_pot(3, 1023) && q.push(4, &[0x90, 60, 1]) && q.push_key(5, 14, false));
        let got: Vec<Message> = std::iter::from_fn(|| q.pop()).collect();
        assert_eq!(got.iter().map(|m| m.ts_us).collect::<Vec<_>>(), [1, 2, 3, 4, 5], "one queue, one order");
        assert_eq!((got[0].kind, got[0].bytes[0], got[0].bytes[1]), (Kind::Key, 40, 1));
        assert_eq!((got[1].kind, got[1].bytes[0], got[1].val), (Kind::Encoder, 6, -3));
        assert_eq!((got[2].kind, got[2].val), (Kind::Pot, 1023));
        assert_eq!((got[3].kind, got[3].len), (Kind::Midi, 3));
        assert_eq!((got[4].kind, got[4].bytes[0], got[4].bytes[1]), (Kind::Key, 14, 0));
    }

    #[test]
    fn wraps_around_many_times() {
        let q = EventQueue::new(16);
        for round in 0..1000u64 {
            for k in 0..11u8 {
                assert!(q.push(round * 16 + k as u64, &[0xB0, k, (round % 128) as u8]));
            }
            for k in 0..11u8 {
                let m = q.pop().unwrap();
                assert_eq!((m.ts_us, m.bytes[1]), (round * 16 + k as u64, k));
            }
        }
        assert_eq!(q.dropped(), 0);
    }

    /// A producer thread and a consumer thread: every message arrives once, in order, with its own contents (a torn read would show as a bad checksum).
    #[test]
    fn two_threads_lose_and_reorder_nothing() {
        let q = Arc::new(EventQueue::new(256));
        let n = 200_000u64;
        let p = q.clone();
        let producer = std::thread::spawn(move || {
            let mut i = 0u64;
            while i < n {
                let b = [(i & 0x7f) as u8, ((i >> 7) & 0x7f) as u8, ((i * 31) & 0x7f) as u8];
                if p.push(i, &b) {
                    i += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut next = 0u64;
        while next < n {
            match q.pop() {
                Some(m) => {
                    assert_eq!(m.ts_us, next, "order");
                    assert_eq!(m.bytes, [(next & 0x7f) as u8, ((next >> 7) & 0x7f) as u8, ((next * 31) & 0x7f) as u8], "contents of {next}");
                    next += 1;
                }
                None => std::thread::yield_now(),
            }
        }
        producer.join().unwrap();
        assert!(q.is_empty());
    }

    #[test]
    fn the_clock_follows_the_samples_and_resyncs_only_on_a_big_drift() {
        let mut c = Clock::new();
        let (s0, per_us) = c.window(100_000, 441, 44_100.0); // 10 ms buffers
        assert!((s0 - 90_000.0).abs() < 1e-6 && (per_us - 0.0441).abs() < 1e-9);
        // a callback that runs 3 ms late or early does not move the window: it still starts where the last one ended
        let (s1, _) = c.window(110_000 + 3_000, 441, 44_100.0);
        assert!((s1 - 100_000.0).abs() < 1e-6, "{s1}");
        let (s2, _) = c.window(120_000 - 3_000, 441, 44_100.0);
        assert!((s2 - 110_000.0).abs() < 1e-6, "{s2}");
        // a stall of half a second pulls the window back to the host's clock
        let (s3, _) = c.window(700_000, 441, 44_100.0);
        assert!((s3 - 690_000.0).abs() < 1e-6, "{s3}");
    }
}

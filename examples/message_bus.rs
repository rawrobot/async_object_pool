//! Message pipeline built on crossbeam_queue and BundledPool.
//!
//! Two backpressure policies when a subscriber queue is full:
//! - `Reject`:    new message is silently dropped.
//! - `DropFront`: oldest message is evicted to make room.
//!
//! Subscribers can register a predicate; only matching messages are delivered.
//! `Subscription` supports both sync `recv()` and async `recv_async()`.
//!
//! Pipeline shape:
//!
//!   main (producer)
//!     └─► Bus ──► Runner (Amplifier) ──► Bus ──► async consumers (filtered)

use asyn_object_pool::{BundledPool, BundledPoolItem, Resettable};
use crossbeam_queue::ArrayQueue;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum SubscriptionStatus {
    Ok,
    Full,
}
use tokio::{sync::Notify, time::sleep};

// ── Message ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
#[allow(dead_code)]
enum Message {
    Sensor { id: u32, value: f64 },
    Processed { source_id: u32, result: f64 },
    Heartbeat,
}

impl Resettable for Message {
    fn reset(&mut self) {
        *self = Message::Heartbeat;
    }
}

type Shared = Arc<BundledPoolItem<Message>>;

// ── Publish result ────────────────────────────────────────────────────────────

/// Returned by `Bus::publish`. `Ok(n)` means all `n` matching subscribers
/// received the message. `Err` means at least one subscriber rejected it
/// (Reject policy + full queue); the counts show what happened.
type PublishResult = Result<usize, PublishError>;

#[derive(Debug)]
struct PublishError {
    delivered: usize,
    /// IDs of subscribers that rejected the message (Reject policy + full queue).
    rejected: Vec<String>,
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "publish incomplete: delivered={}, rejected by: [{}]",
            self.delivered,
            self.rejected.join(", ")
        )
    }
}

impl std::error::Error for PublishError {}

// ── Subscription ──────────────────────────────────────────────────────────────

struct Subscription {
    id: String,
    queue: Arc<ArrayQueue<Shared>>,
    notify: Arc<Notify>,
    /// Incremented by the bus each time this subscriber's queue was full.
    /// Shared with the Subscriber slot so publish() can update it.
    pressure: Arc<AtomicU64>,
}

impl Subscription {
    /// Real-time status derived from the current queue state.
    fn status(&self) -> SubscriptionStatus {
        if self.queue.len() == self.queue.capacity() {
            SubscriptionStatus::Full
        } else {
            SubscriptionStatus::Ok
        }
    }

    /// Total number of times the bus found this queue at capacity.
    fn pressure(&self) -> u64 {
        self.pressure.load(Ordering::Relaxed)
    }
}

impl fmt::Display for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Subscription({}, status={:?}, pressure={})",
            self.id,
            self.status(),
            self.pressure()
        )
    }
}

impl Subscription {
    fn recv(&self) -> Option<Shared> {
        self.queue.pop()
    }

    /// Wait until a message is available, then return it.
    ///
    /// `enable()` is called before the queue check so that a notification
    /// arriving between the check and `.await` is never lost.
    async fn recv_async(&self) -> Shared {
        loop {
            let notified = self.notify.notified();
            // Safety against the publish/check race: register interest first,
            // then inspect the queue.
            tokio::pin!(notified);
            notified.as_mut().enable();

            if let Some(msg) = self.queue.pop() {
                return msg;
            }
            notified.await;
        }
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.queue.len()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Iterator for Subscription {
    type Item = Shared;
    fn next(&mut self) -> Option<Shared> {
        self.queue.pop()
    }
}

// ── Bus ───────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum FullPolicy {
    /// Drop new messages when a subscriber queue is full.
    Reject,
    /// Evict the oldest message to make room for the new one.
    DropFront,
}

struct Subscriber {
    id: String,
    queue: Arc<ArrayQueue<Shared>>,
    filter: Box<dyn Fn(&Message) -> bool + Send + Sync + 'static>,
    notify: Arc<Notify>,
    pressure: Arc<AtomicU64>,
}

struct Bus {
    subscribers: Vec<Subscriber>,
    policy: FullPolicy,
    dropped: AtomicU64,
    rejected: AtomicU64,
}

impl Bus {
    fn new(policy: FullPolicy) -> Self {
        Self {
            subscribers: vec![],
            policy,
            dropped: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// Subscribe with a predicate; only messages where `filter(msg)` is true are delivered.
    /// Must be called before wrapping the bus in `Arc`.
    fn subscribe(
        &mut self,
        id: impl Into<String>,
        capacity: usize,
        filter: impl Fn(&Message) -> bool + Send + Sync + 'static,
    ) -> Subscription {
        let id = id.into();
        let queue = Arc::new(ArrayQueue::new(capacity));
        let notify = Arc::new(Notify::new());
        let pressure = Arc::new(AtomicU64::new(0));
        self.subscribers.push(Subscriber {
            id: id.clone(),
            queue: Arc::clone(&queue),
            filter: Box::new(filter),
            notify: Arc::clone(&notify),
            pressure: Arc::clone(&pressure),
        });
        Subscription {
            id,
            queue,
            notify,
            pressure,
        }
    }

    /// Subscribe to all messages (no filter).
    fn subscribe_all(&mut self, id: impl Into<String>, capacity: usize) -> Subscription {
        self.subscribe(id, capacity, |_| true)
    }

    fn publish(&self, msg: Shared) -> PublishResult {
        let mut delivered = 0;
        let mut rejected: Vec<String> = vec![];

        for sub in &self.subscribers {
            if !(sub.filter)(&msg) {
                continue;
            }
            let q = &sub.queue;
            let pushed = match self.policy {
                FullPolicy::Reject => {
                    if q.push(Arc::clone(&msg)).is_err() {
                        self.rejected.fetch_add(1, Ordering::Relaxed);
                        sub.pressure.fetch_add(1, Ordering::Relaxed);
                        rejected.push(sub.id.clone());
                        false
                    } else {
                        true
                    }
                }
                FullPolicy::DropFront => {
                    match q.push(Arc::clone(&msg)) {
                        Ok(()) => true,
                        Err(m) => {
                            q.pop(); // evict oldest; Arc drops → pool reclaims when last ref gone
                            self.dropped.fetch_add(1, Ordering::Relaxed);
                            sub.pressure.fetch_add(1, Ordering::Relaxed);
                            let _ = q.push(m);
                            true
                        }
                    }
                }
            };
            if pushed {
                delivered += 1;
                sub.notify.notify_one();
            }
        }

        if rejected.is_empty() {
            Ok(delivered)
        } else {
            Err(PublishError {
                delivered,
                rejected,
            })
        }
    }

    #[allow(dead_code)]
    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    #[allow(dead_code)]
    fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }
}

// ── Runner ────────────────────────────────────────────────────────────────────

trait Processor: Send + 'static {
    fn process(&self, input: &Message, output: &mut Message);
}

struct Runner<P: Processor> {
    processor: P,
    pool: BundledPool<Message>,
    inbox: Subscription,
    outbus: Arc<Bus>,
    stop: Arc<AtomicBool>,
}

impl<P: Processor> Runner<P> {
    fn spawn(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            while !self.stop.load(Ordering::Relaxed) || !self.inbox.is_empty() {
                if let Some(input) = self.inbox.recv() {
                    let mut output = self.pool.take();
                    self.processor.process(&input, &mut output);
                    if let Err(e) = self.outbus.publish(output.into_arc()) {
                        eprintln!("[runner] publish error: {e}");
                    }
                    // input Arc drops here → pool reclaims when all subscribers release
                } else {
                    thread::sleep(Duration::from_micros(100));
                }
            }
        })
    }
}

// ── Processors ────────────────────────────────────────────────────────────────

struct Amplifier {
    factor: f64,
}

impl Processor for Amplifier {
    fn process(&self, input: &Message, output: &mut Message) {
        *output = match input {
            Message::Sensor { id, value } => Message::Processed {
                source_id: *id,
                result: value * self.factor,
            },
            _ => Message::Heartbeat,
        };
    }
}

// ── Demo ──────────────────────────────────────────────────────────────────────

// Send ids 0-9, factor 2.0 → results 0, 2, 4, 6, 8, 10, 12, 14, 16, 18
//   even filter (source_id % 2 == 0): ids 0, 2, 4, 6, 8       → 5 messages
//   high filter (result > 10.0):      ids 6, 7, 8, 9           → 4 messages

#[tokio::main]
async fn main() {
    let pool = BundledPool::new(4, 32, || Message::Heartbeat);

    // Large queue — no drops in this demo so counts are predictable
    let mut in_bus = Bus::new(FullPolicy::Reject);
    let runner_inbox = in_bus.subscribe_all("runner", 32);
    let in_bus = Arc::new(in_bus);

    let mut out_bus = Bus::new(FullPolicy::Reject);
    let even_sub = out_bus.subscribe(
        "even-consumer",
        32,
        |m| matches!(m, Message::Processed { source_id, .. } if source_id % 2 == 0),
    );
    let high_sub = out_bus.subscribe(
        "high-consumer",
        32,
        |m| matches!(m, Message::Processed { result, .. } if *result > 10.0),
    );
    let out_bus = Arc::new(out_bus);

    let stop = Arc::new(AtomicBool::new(false));

    // Sync runner in its own OS thread (crossbeam ops are non-blocking)
    let runner_handle = Runner {
        processor: Amplifier { factor: 2.0 },
        pool: pool.clone(),
        inbox: runner_inbox,
        outbus: Arc::clone(&out_bus),
        stop: Arc::clone(&stop),
    }
    .spawn();

    // Async consumers — each awaits messages as they arrive
    let even_task = tokio::spawn(async move {
        println!("[even] receiving 5 messages:");
        for _ in 0..5 {
            let msg = even_sub.recv_async().await;
            println!("[even]   {:?}", **msg);
        }
        println!("[even] final status: {even_sub}");
    });

    let high_task = tokio::spawn(async move {
        println!("[high] receiving 4 messages:");
        for _ in 0..4 {
            let msg = high_sub.recv_async().await;
            println!("[high]   {:?}", **msg);
        }
        println!("[high] final status: {high_sub}");
    });

    // Produce messages one per millisecond so consumers visibly interleave
    for i in 0..10u32 {
        let mut msg = pool.take();
        *msg = Message::Sensor {
            id: i,
            value: i as f64,
        };
        if let Err(e) = in_bus.publish(msg.into_arc()) {
            eprintln!("[main] publish error: {e}");
        }
        sleep(Duration::from_millis(1)).await;
    }

    even_task.await.unwrap();
    high_task.await.unwrap();

    stop.store(true, Ordering::Relaxed);
    runner_handle.join().unwrap();
}

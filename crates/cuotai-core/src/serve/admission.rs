//! PRD 0050 RF5: the work a connection may cause before it is authenticated.
//!
//! [`Pending`] counts the connections still sending their head, in all and
//! per peer address, so one peer (all of loopback is one peer, by design)
//! holding slow heads cannot take every slot. A connection over either
//! limit waits, first come first served and for at most the head deadline,
//! for a slot that frees ([`PendingSlot::enter_or_queue`]), so a client
//! that keeps refreshing half-open sockets queues behind the clients that
//! were already waiting instead of taking every freed slot. [`Buckets`]
//! rate-limits the completed heads per peer and in all; the server keeps
//! one set for admitted heads and another for refused ones, so refused
//! requests never spend the budget of the clients that send the token.
//! The buckets are pure: time is passed in (`tokio::time::Instant`, so
//! tests can pause the clock).

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::Instant;

use super::Rate;

/// Peers a [`Buckets`] remembers at most; the least recently seen is
/// forgotten first.
pub const MAX_PEERS: usize = 256;

/// How long a [`Buckets`] remembers an idle peer.
pub const PEER_MEMORY: Duration = Duration::from_secs(600);

/// Connections still reading their head, and those waiting for a slot.
#[derive(Debug, Default)]
pub struct Pending {
    total: usize,
    per_peer: HashMap<IpAddr, usize>,
    waiting: VecDeque<Waiter>,
    next_id: u64,
}

/// A connection waiting for a pending slot, with the limits of its accept.
#[derive(Debug)]
struct Waiter {
    id: u64,
    peer: IpAddr,
    max: usize,
    max_per_peer: usize,
    granted: oneshot::Sender<()>,
}

impl Pending {
    /// Count a new head of `peer`, unless that would pass `max` in all or
    /// `max_per_peer` for it.
    pub fn try_enter(&mut self, peer: IpAddr, max: usize, max_per_peer: usize) -> bool {
        let held = self.per_peer.get(&peer).copied().unwrap_or(0);
        if self.total >= max || held >= max_per_peer {
            return false;
        }
        self.total += 1;
        self.per_peer.insert(peer, held + 1);
        true
    }

    /// Give a slot of `peer` back and hand it to the oldest waiter that
    /// fits.
    pub fn leave(&mut self, peer: IpAddr) {
        self.release(peer);
        self.hand_off();
    }

    fn release(&mut self, peer: IpAddr) {
        self.total = self.total.saturating_sub(1);
        if let Some(held) = self.per_peer.get_mut(&peer) {
            *held -= 1;
            if *held == 0 {
                self.per_peer.remove(&peer);
            }
        }
    }

    /// Grant free slots to the waiters, oldest first; a waiter whose peer
    /// is still at its limit keeps its place, and one that gave up leaves.
    fn hand_off(&mut self) {
        let mut at = 0;
        while at < self.waiting.len() {
            let (peer, max, max_per_peer, gone) = {
                let waiter = &self.waiting[at];
                (
                    waiter.peer,
                    waiter.max,
                    waiter.max_per_peer,
                    waiter.granted.is_closed(),
                )
            };
            if gone {
                self.waiting.remove(at);
            } else if self.try_enter(peer, max, max_per_peer) {
                let waiter = self.waiting.remove(at).expect("index in range");
                if waiter.granted.send(()).is_err() {
                    self.release(peer);
                }
            } else {
                at += 1;
            }
        }
    }

    /// Whether a live waiter of `peer` is queued, so a newcomer of the same
    /// peer must queue behind it.
    fn peer_waiting(&self, peer: IpAddr) -> bool {
        self.waiting
            .iter()
            .any(|waiter| waiter.peer == peer && !waiter.granted.is_closed())
    }

    #[cfg(test)]
    pub fn total(&self) -> usize {
        self.total
    }
}

/// A [`Pending`] slot, given back when dropped.
pub struct PendingSlot {
    pending: Arc<Mutex<Pending>>,
    peer: IpAddr,
}

/// What [`PendingSlot::enter_or_queue`] gives a new connection.
pub enum Entry {
    /// A slot, now.
    Slot(PendingSlot),
    /// A place in the queue; [`Ticket::wait`] for the slot.
    Queued(Ticket),
    /// The queue is full too: close the connection.
    Full,
}

/// A connection's place in the [`Pending`] queue.
pub struct Ticket {
    pending: Arc<Mutex<Pending>>,
    peer: IpAddr,
    id: u64,
    granted: oneshot::Receiver<()>,
}

impl Ticket {
    /// The slot, once one is handed to this ticket within `limit`; `None`
    /// when the limit passes first (the place in the queue is given up).
    pub async fn wait(mut self, limit: Duration) -> Option<PendingSlot> {
        let slot = |ticket: &Self| PendingSlot {
            pending: Arc::clone(&ticket.pending),
            peer: ticket.peer,
        };
        if let Ok(Ok(())) = tokio::time::timeout(limit, &mut self.granted).await {
            return Some(slot(&self));
        }
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting
            .retain(|waiter| waiter.id != self.id);
        // Granted between the deadline and the lock: the slot is counted.
        self.granted.try_recv().is_ok().then(|| slot(&self))
    }
}

impl PendingSlot {
    /// PRD 0050 RF5.2: a slot of `peer` now, or a place in the queue while
    /// fewer than `max_waiting` connections wait, or `Full`. A newcomer
    /// queues behind a waiter of its own peer even if a slot is free.
    pub fn enter_or_queue(
        pending: &Arc<Mutex<Pending>>,
        peer: IpAddr,
        max: usize,
        max_per_peer: usize,
        max_waiting: usize,
    ) -> Entry {
        let mut guard = pending.lock().unwrap_or_else(PoisonError::into_inner);
        guard.waiting.retain(|waiter| !waiter.granted.is_closed());
        if !guard.peer_waiting(peer) && guard.try_enter(peer, max, max_per_peer) {
            return Entry::Slot(Self {
                pending: Arc::clone(pending),
                peer,
            });
        }
        if guard.waiting.len() >= max_waiting {
            return Entry::Full;
        }
        let (granted, receiver) = oneshot::channel();
        guard.next_id += 1;
        let id = guard.next_id;
        guard.waiting.push_back(Waiter {
            id,
            peer,
            max,
            max_per_peer,
            granted,
        });
        Entry::Queued(Ticket {
            pending: Arc::clone(pending),
            peer,
            id,
            granted: receiver,
        })
    }

    #[cfg(test)]
    pub fn enter(
        pending: &Arc<Mutex<Pending>>,
        peer: IpAddr,
        max: usize,
        max_per_peer: usize,
    ) -> Option<Self> {
        pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_enter(peer, max, max_per_peer)
            .then(|| Self {
                pending: Arc::clone(pending),
                peer,
            })
    }
}

impl Drop for PendingSlot {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .leave(self.peer);
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn full(rate: Rate, now: Instant) -> Self {
        Self {
            tokens: f64::from(rate.burst),
            at: now,
        }
    }

    fn refill(&mut self, rate: Rate, now: Instant) {
        let per_second = f64::from(rate.per_minute) / 60.0;
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * per_second).min(f64::from(rate.burst));
        self.at = now;
    }

    /// The wait until one token is back, when none is left now.
    fn wait(&self, rate: Rate) -> Option<Duration> {
        if self.tokens >= 1.0 {
            return None;
        }
        let per_second = f64::from(rate.per_minute).max(1.0) / 60.0;
        Some(Duration::from_secs_f64((1.0 - self.tokens) / per_second))
    }
}

/// Token buckets of completed heads: one per peer and one for all.
#[derive(Debug, Default)]
pub struct Buckets {
    peers: HashMap<IpAddr, Bucket>,
    global: Option<Bucket>,
}

impl Buckets {
    /// Spend one token of `peer` and one of the global bucket at `now`, or
    /// say how long until both have one (neither is spent then).
    pub fn take(
        &mut self,
        peer: IpAddr,
        peer_rate: Rate,
        global_rate: Rate,
        now: Instant,
    ) -> Result<(), Duration> {
        self.forget_idle(now);
        let global = self
            .global
            .get_or_insert_with(|| Bucket::full(global_rate, now));
        global.refill(global_rate, now);
        let global_wait = global.wait(global_rate);
        let own = self
            .peers
            .entry(peer)
            .or_insert_with(|| Bucket::full(peer_rate, now));
        own.refill(peer_rate, now);
        let own_wait = own.wait(peer_rate);
        if let Some(wait) = own_wait.into_iter().chain(global_wait).max() {
            return Err(wait);
        }
        own.tokens -= 1.0;
        if let Some(global) = self.global.as_mut() {
            global.tokens -= 1.0;
        }
        Ok(())
    }

    fn forget_idle(&mut self, now: Instant) {
        self.peers
            .retain(|_, bucket| now.saturating_duration_since(bucket.at) < PEER_MEMORY);
        while self.peers.len() >= MAX_PEERS {
            let Some(oldest) = self
                .peers
                .iter()
                .min_by_key(|(_, bucket)| bucket.at)
                .map(|(peer, _)| *peer)
            else {
                break;
            };
            self.peers.remove(&oldest);
        }
    }

    #[cfg(test)]
    pub fn peers(&self) -> usize {
        self.peers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last))
    }

    const PEER: Rate = Rate {
        per_minute: 60,
        burst: 20,
    };
    const GLOBAL: Rate = Rate {
        per_minute: 240,
        burst: 40,
    };

    // security-invariant: SEC-SERVE-ADMISSION
    #[test]
    fn one_peer_holding_its_pending_slots_leaves_room_for_another() {
        let mut pending = Pending::default();
        for _ in 0..4 {
            assert!(pending.try_enter(ip(1), 16, 4));
        }
        assert!(!pending.try_enter(ip(1), 16, 4), "per-peer cap");
        assert!(pending.try_enter(ip(2), 16, 4), "another peer is served");
        pending.leave(ip(1));
        assert!(pending.try_enter(ip(1), 16, 4), "a freed slot is reused");
        // The global cap holds across peers.
        let mut full = Pending::default();
        for peer in 0..4 {
            for _ in 0..4 {
                assert!(full.try_enter(ip(peer), 16, 4));
            }
        }
        assert!(!full.try_enter(ip(9), 16, 4));
        assert_eq!(full.total(), 16);
    }

    /// PRD 0050 RF5.2: over the limit a connection waits for a freed slot,
    /// first come first served; a newcomer of the same peer queues behind
    /// the waiters, another peer with room is not held up, and a full queue
    /// closes the connection.
    #[tokio::test]
    async fn freed_slots_go_to_the_oldest_waiter_and_the_queue_is_bounded() {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let enter = |peer| PendingSlot::enter_or_queue(&pending, peer, 16, 2, 2);
        let mut held: Vec<_> = (0..2)
            .map(|_| match enter(ip(1)) {
                Entry::Slot(slot) => slot,
                _ => panic!("room for two"),
            })
            .collect();
        let Entry::Queued(first) = enter(ip(1)) else {
            panic!("over the peer limit it queues");
        };
        assert!(
            matches!(enter(ip(2)), Entry::Slot(_)),
            "another peer has room"
        );
        let Entry::Queued(second) = enter(ip(1)) else {
            panic!("queues behind the first waiter");
        };
        assert!(matches!(enter(ip(1)), Entry::Full), "the queue holds two");

        drop(held.pop());
        let first = first.wait(Duration::from_millis(50)).await;
        assert!(first.is_some(), "the oldest waiter gets the freed slot");
        assert!(
            second.wait(Duration::from_millis(50)).await.is_none(),
            "the next one gives up at its deadline"
        );
        assert_eq!(pending.lock().unwrap().waiting.len(), 0);
        assert!(
            matches!(enter(ip(1)), Entry::Queued(_)),
            "both slots of the peer are still held"
        );
        drop((held, first));
        assert_eq!(pending.lock().unwrap().total(), 0);
        assert!(matches!(enter(ip(1)), Entry::Slot(_)));
    }

    #[test]
    fn slots_are_given_back_when_dropped() {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let slots: Vec<_> = (0..4)
            .map(|_| PendingSlot::enter(&pending, ip(1), 16, 4).unwrap())
            .collect();
        assert!(PendingSlot::enter(&pending, ip(1), 16, 4).is_none());
        drop(slots);
        assert_eq!(pending.lock().unwrap().total(), 0);
        assert!(PendingSlot::enter(&pending, ip(1), 16, 4).is_some());
    }

    #[test]
    fn a_peer_gets_its_burst_then_one_head_a_second() {
        let start = Instant::now();
        let mut buckets = Buckets::default();
        for _ in 0..20 {
            assert!(buckets.take(ip(1), PEER, GLOBAL, start).is_ok());
        }
        let wait = buckets.take(ip(1), PEER, GLOBAL, start).unwrap_err();
        assert!(
            wait <= Duration::from_secs(1) && wait > Duration::ZERO,
            "{wait:?}"
        );
        // Another peer still has its own burst.
        assert!(buckets.take(ip(2), PEER, GLOBAL, start).is_ok());
        // One second later, one more head.
        let later = start + Duration::from_secs(1);
        assert!(buckets.take(ip(1), PEER, GLOBAL, later).is_ok());
        assert!(buckets.take(ip(1), PEER, GLOBAL, later).is_err());
        // After a minute the burst is back, never more.
        let refilled = start + Duration::from_secs(120);
        for _ in 0..20 {
            assert!(buckets.take(ip(1), PEER, GLOBAL, refilled).is_ok());
        }
        assert!(buckets.take(ip(1), PEER, GLOBAL, refilled).is_err());
    }

    #[test]
    fn the_global_bucket_bounds_many_peers() {
        let start = Instant::now();
        let mut buckets = Buckets::default();
        let mut served = 0;
        for peer in 0..10 {
            for _ in 0..10 {
                if buckets.take(ip(peer), PEER, GLOBAL, start).is_ok() {
                    served += 1;
                }
            }
        }
        assert_eq!(served, 40, "global burst");
        // A refused head spends nothing: the peer that never got one still
        // has its whole burst once the global bucket refills.
        let later = start + Duration::from_secs(10);
        assert!(buckets.take(ip(9), PEER, GLOBAL, later).is_ok());
    }

    #[test]
    fn idle_peers_are_forgotten_and_at_most_256_are_kept() {
        let start = Instant::now();
        let mut buckets = Buckets::default();
        let wide = Rate {
            per_minute: 60_000,
            burst: 10_000,
        };
        for n in 0..300u32 {
            let peer = IpAddr::V4(Ipv4Addr::from(0x0A00_0000 + n));
            buckets
                .take(
                    peer,
                    PEER,
                    wide,
                    start + Duration::from_millis(u64::from(n)),
                )
                .unwrap();
        }
        assert!(buckets.peers() <= MAX_PEERS);
        buckets
            .take(
                ip(1),
                PEER,
                wide,
                start + PEER_MEMORY + Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(buckets.peers(), 1, "ten minutes idle are forgotten");
    }
}

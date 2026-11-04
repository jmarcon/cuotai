//! PRD 0050 RF5: the work a connection may cause before it is authenticated.
//!
//! [`Pending`] counts the connections still sending their head, in all and
//! per peer address, so one peer (all of loopback is one peer, by design)
//! holding slow heads cannot take every slot. [`Buckets`] rate-limits the
//! completed heads per peer and in all, before any validation or export.
//! Both are pure: time is passed in (`tokio::time::Instant`, so tests can
//! pause the clock).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use super::Rate;

/// Peers a [`Buckets`] remembers at most; the least recently seen is
/// forgotten first.
pub const MAX_PEERS: usize = 256;

/// How long a [`Buckets`] remembers an idle peer.
pub const PEER_MEMORY: Duration = Duration::from_secs(600);

/// Connections still reading their head.
#[derive(Debug, Default)]
pub struct Pending {
    total: usize,
    per_peer: HashMap<IpAddr, usize>,
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

    pub fn leave(&mut self, peer: IpAddr) {
        self.total = self.total.saturating_sub(1);
        if let Some(held) = self.per_peer.get_mut(&peer) {
            *held -= 1;
            if *held == 0 {
                self.per_peer.remove(&peer);
            }
        }
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

impl PendingSlot {
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

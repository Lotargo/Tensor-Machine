use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::Instant;

pub const CB_CLOSED: u8 = 0;
pub const CB_OPEN: u8 = 1;
pub const CB_HALF_OPEN: u8 = 2;

pub struct CircuitBreaker {
    failures: AtomicU32,
    last_failure: RwLock<Option<Instant>>,
    threshold: u32,
    reset_timeout_secs: u64,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_timeout_secs: u64) -> Self {
        Self {
            failures: AtomicU32::new(0),
            last_failure: RwLock::new(None),
            threshold,
            reset_timeout_secs,
        }
    }

    pub fn is_open(&self) -> bool {
        if self.failures.load(Ordering::SeqCst) >= self.threshold {
            let last = *self.last_failure.read().unwrap();
            if let Some(time) = last {
                return time.elapsed().as_secs() < self.reset_timeout_secs;
            }
            return true;
        }
        false
    }

    /// Returns the circuit breaker state: 0=Closed, 1=Open, 2=Half-Open.
    pub fn state(&self) -> u8 {
        if self.failures.load(Ordering::SeqCst) < self.threshold {
            return CB_CLOSED;
        }
        let last = *self.last_failure.read().unwrap();
        match last {
            Some(time) if time.elapsed().as_secs() >= self.reset_timeout_secs => CB_HALF_OPEN,
            _ => CB_OPEN,
        }
    }

    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_failure.write().unwrap();
        *last = Some(Instant::now());
    }

    pub fn record_success(&self) {
        let mut last = self.last_failure.write().unwrap();
        *last = None;
        self.failures.store(0, Ordering::SeqCst);
    }
}

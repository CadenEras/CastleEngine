use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cumulative limits applied to one document or one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub max_decoded_bytes: u64,
    pub max_objects_visited: u64,
    pub max_nesting_depth: u32,
}

impl Default for ResourceLimits {
    /// Conservative starting values, to be calibrated with the P0 corpus.
    fn default() -> Self {
        Self {
            max_decoded_bytes: 256 * 1024 * 1024,
            max_objects_visited: 1_000_000,
            max_nesting_depth: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    DecodedBytes,
    ObjectsVisited,
    NestingDepth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    LimitExceeded { kind: LimitKind, limit: u64 },
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { kind, limit } => {
                write!(f, "resource limit exceeded: {kind:?} (limit {limit})")
            }
            Self::Cancelled => f.write_str("operation cancelled"),
        }
    }
}

impl std::error::Error for Error {}

/// Cooperative cancellation flag shared between a requester and a running task.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Running consumption against `ResourceLimits`. A refused charge leaves the
/// counters unchanged.
#[derive(Debug)]
pub struct Budget {
    limits: ResourceLimits,
    decoded_bytes: u64,
    objects_visited: u64,
    cancel: CancellationToken,
}

impl Budget {
    pub fn new(limits: ResourceLimits, cancel: CancellationToken) -> Self {
        Self {
            limits,
            decoded_bytes: 0,
            objects_visited: 0,
            cancel,
        }
    }

    pub fn limits(&self) -> ResourceLimits {
        self.limits
    }

    pub fn decoded_bytes(&self) -> u64 {
        self.decoded_bytes
    }

    pub fn objects_visited(&self) -> u64 {
        self.objects_visited
    }

    pub fn check_cancelled(&self) -> Result<(), Error> {
        if self.cancel.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn charge_decoded_bytes(&mut self, amount: u64) -> Result<(), Error> {
        charge(
            &mut self.decoded_bytes,
            amount,
            self.limits.max_decoded_bytes,
            LimitKind::DecodedBytes,
        )
    }

    pub fn charge_object(&mut self) -> Result<(), Error> {
        charge(
            &mut self.objects_visited,
            1,
            self.limits.max_objects_visited,
            LimitKind::ObjectsVisited,
        )
    }

    pub fn check_depth(&self, depth: u32) -> Result<(), Error> {
        if depth > self.limits.max_nesting_depth {
            Err(Error::LimitExceeded {
                kind: LimitKind::NestingDepth,
                limit: u64::from(self.limits.max_nesting_depth),
            })
        } else {
            Ok(())
        }
    }
}

fn charge(counter: &mut u64, amount: u64, limit: u64, kind: LimitKind) -> Result<(), Error> {
    let next = counter
        .checked_add(amount)
        .filter(|total| *total <= limit)
        .ok_or(Error::LimitExceeded { kind, limit })?;
    *counter = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(max_decoded_bytes: u64, max_objects_visited: u64) -> Budget {
        let limits = ResourceLimits {
            max_decoded_bytes,
            max_objects_visited,
            max_nesting_depth: 2,
        };
        Budget::new(limits, CancellationToken::new())
    }

    #[test]
    fn exact_limit_is_accepted_and_limit_plus_one_refused() {
        let mut b = budget(10, 2);
        assert_eq!(b.charge_decoded_bytes(10), Ok(()));
        assert_eq!(
            b.charge_decoded_bytes(1),
            Err(Error::LimitExceeded {
                kind: LimitKind::DecodedBytes,
                limit: 10
            })
        );
        assert_eq!(b.decoded_bytes(), 10);

        assert_eq!(b.charge_object(), Ok(()));
        assert_eq!(b.charge_object(), Ok(()));
        assert_eq!(
            b.charge_object(),
            Err(Error::LimitExceeded {
                kind: LimitKind::ObjectsVisited,
                limit: 2
            })
        );
        assert_eq!(b.objects_visited(), 2);
    }

    #[test]
    fn refused_charge_leaves_counter_unchanged() {
        let mut b = budget(10, 1);
        assert_eq!(b.charge_decoded_bytes(4), Ok(()));
        assert!(b.charge_decoded_bytes(7).is_err());
        assert_eq!(b.decoded_bytes(), 4);
        assert_eq!(b.charge_decoded_bytes(6), Ok(()));
    }

    #[test]
    fn overflowing_charge_is_refused() {
        let mut b = budget(u64::MAX, 1);
        assert_eq!(b.charge_decoded_bytes(u64::MAX), Ok(()));
        assert!(b.charge_decoded_bytes(1).is_err());
        assert_eq!(b.decoded_bytes(), u64::MAX);
    }

    #[test]
    fn depth_limit_is_inclusive() {
        let b = budget(1, 1);
        assert_eq!(b.check_depth(2), Ok(()));
        assert_eq!(
            b.check_depth(3),
            Err(Error::LimitExceeded {
                kind: LimitKind::NestingDepth,
                limit: 2
            })
        );
    }

    #[test]
    fn cancellation_is_visible_through_clones() {
        let token = CancellationToken::new();
        let b = Budget::new(ResourceLimits::default(), token.clone());
        assert_eq!(b.check_cancelled(), Ok(()));
        token.cancel();
        assert_eq!(b.check_cancelled(), Err(Error::Cancelled));
    }
}

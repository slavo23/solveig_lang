//! Scheduler-owned timer registrations and monotonic clock state.
//! Real execution measures elapsed host time, while deterministic execution
//! advances a logical clock only when the run queue is empty.

use super::ProcessId;
use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Identifies a monotonic millisecond instant within one runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TimerEntry {
    deadline: Deadline,
    process: ProcessId,
}

enum TimerClock {
    Real { started: Instant },
    Virtual { now: Deadline },
}

pub(super) struct TimerQueue {
    clock: TimerClock,
    entries: BTreeSet<TimerEntry>,
}

impl TimerQueue {
    pub(super) fn real_time() -> Self {
        Self {
            clock: TimerClock::Real {
                started: Instant::now(),
            },
            entries: BTreeSet::new(),
        }
    }

    pub(super) fn virtual_time() -> Self {
        Self {
            clock: TimerClock::Virtual { now: Deadline(0) },
            entries: BTreeSet::new(),
        }
    }

    pub(super) fn register(&mut self, process: ProcessId, milliseconds: u64) -> Deadline {
        let deadline = Deadline(self.now().0.saturating_add(milliseconds));
        if !self.entries.insert(TimerEntry { deadline, process }) {
            solveig_base::ice!("process registered the same timer twice")
        }
        deadline
    }

    pub(super) fn cancel(&mut self, process: ProcessId, deadline: Deadline) {
        if !self.entries.remove(&TimerEntry { deadline, process }) {
            solveig_base::ice!("blocked timer process had no matching registration")
        }
    }

    pub(super) fn pop_due(&mut self) -> Option<(Deadline, ProcessId)> {
        let entry = self.entries.first().copied()?;
        if entry.deadline > self.now() {
            return None;
        }
        let Some(entry) = self.entries.pop_first() else {
            solveig_base::ice!("timer queue head disappeared while waking due timers")
        };
        Some((entry.deadline, entry.process))
    }

    pub(super) fn advance_virtual_to_next(&mut self) -> bool {
        let Some(deadline) = self.entries.first().map(|entry| entry.deadline) else {
            return false;
        };
        let TimerClock::Virtual { now } = &mut self.clock else {
            return false;
        };
        *now = deadline;
        true
    }

    pub(super) fn wait_until_next(&self) -> Option<Duration> {
        let deadline = self.entries.first()?.deadline;
        Some(Duration::from_millis(
            deadline.0.saturating_sub(self.now().0),
        ))
    }

    /// Returns nanoseconds from this runtime's monotonic origin.
    pub(super) fn monotonic_nanos(&self) -> i64 {
        duration_to_nanos(self.elapsed())
    }

    /// Returns signed nanoseconds from the Unix epoch.
    pub(super) fn system_nanos(&self) -> i64 {
        match &self.clock {
            TimerClock::Real { .. } => system_time_to_nanos(SystemTime::now()),
            TimerClock::Virtual { now } => duration_to_nanos(Duration::from_millis(now.0)),
        }
    }

    fn now(&self) -> Deadline {
        Deadline(u64::try_from(self.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    fn elapsed(&self) -> Duration {
        match &self.clock {
            TimerClock::Real { started } => started.elapsed(),
            TimerClock::Virtual { now } => Duration::from_millis(now.0),
        }
    }
}

fn duration_to_nanos(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
}

fn system_time_to_nanos(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration_to_nanos(duration),
        Err(error) => {
            i64::try_from(error.duration().as_nanos()).map_or(i64::MIN, i64::saturating_neg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_monotonic_time_never_decreases() {
        let timers = TimerQueue::real_time();
        let before = timers.monotonic_nanos();
        let after = timers.monotonic_nanos();
        assert!(after >= before);
    }

    #[test]
    fn system_time_conversion_is_signed_and_saturating() {
        assert_eq!(system_time_to_nanos(UNIX_EPOCH), 0);
        assert_eq!(duration_to_nanos(Duration::from_nanos(u64::MAX)), i64::MAX);
        assert_eq!(
            system_time_to_nanos(UNIX_EPOCH + Duration::from_nanos(7)),
            7
        );
        assert_eq!(
            system_time_to_nanos(UNIX_EPOCH - Duration::from_nanos(7)),
            -7
        );
    }
}

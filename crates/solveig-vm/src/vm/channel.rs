//! Typed channel operations and scheduler-owned FIFO waiter queues.
//! Channel values live in the managed heap; blocked process state remains in
//! the runtime scheduler so all suspension uses the existing process machine.

use super::*;
use crate::heap::{Chan, PayloadStore};

pub(super) enum ChannelOutcome {
    Ready(Value),
    Empty,
    Closed,
    Blocked,
}

impl Interpreter<'_> {
    pub(super) fn alloc_channel(&mut self, capacity: Option<u32>) -> Value {
        self.safe_point();
        self.heap.alloc(HeapObject::Chan(Chan {
            queue: PayloadStore::empty(),
            capacity,
            closed: false,
        }))
    }

    pub(super) fn expect_channel_id(
        &self,
        value: Value,
        builtin: &str,
    ) -> Result<ChanId, RuntimeError> {
        let Value::Obj(id) = value else {
            return Err(RuntimeError::builtin(format!(
                "{builtin} expected a channel"
            )));
        };
        let id = ChanId(id);
        self.heap
            .channel(id)
            .map(|_| id)
            .ok_or_else(|| RuntimeError::builtin(format!("{builtin} expected a channel")))
    }

    /// Sends through the scheduler-owned blocking primitive described by
    /// Peyton Jones, Gordon, and Finne, "Concurrent Haskell" (POPL 1996).
    pub(super) fn send_channel(&mut self, id: ChanId, value: Value) -> ChannelOutcome {
        let Some(channel) = self.heap.channel(id) else {
            solveig_base::ice!("validated channel identity no longer names a channel")
        };
        if channel.closed {
            return ChannelOutcome::Closed;
        }

        if let Some(receiver) = self.pop_receiver(id) {
            self.wake_channel_process(receiver, id, ChannelResult::Value(value));
            return ChannelOutcome::Ready(Value::Unit);
        }

        let full = channel.capacity.is_some_and(|capacity| {
            channel.queue.len()
                >= usize::try_from(capacity)
                    .unwrap_or_else(|_| solveig_base::ice!("channel capacity exceeds usize"))
        });
        if !full {
            self.heap.push_channel(id, value);
            return ChannelOutcome::Ready(Value::Unit);
        }

        let process_id = self.id;
        self.channel_waiters
            .entry(id)
            .or_default()
            .senders
            .push_back((process_id, value));
        self.park_on_channel(id);
        ChannelOutcome::Blocked
    }

    pub(super) fn recv_channel(&mut self, id: ChanId, block: bool) -> ChannelOutcome {
        let Some(channel) = self.heap.channel(id) else {
            solveig_base::ice!("validated channel identity no longer names a channel")
        };

        if let Some(value) = self.heap.pop_channel(id) {
            self.admit_waiting_sender(id);
            return ChannelOutcome::Ready(value);
        }
        if channel.closed {
            return ChannelOutcome::Closed;
        }
        if !block {
            return ChannelOutcome::Empty;
        }

        let process_id = self.id;
        self.channel_waiters
            .entry(id)
            .or_default()
            .receivers
            .push_back(process_id);
        self.park_on_channel(id);
        ChannelOutcome::Blocked
    }

    pub(super) fn close_channel_value(&mut self, id: ChanId) -> ChannelOutcome {
        let Some(channel) = self.heap.channel(id) else {
            solveig_base::ice!("validated channel identity no longer names a channel")
        };
        if channel.closed {
            return ChannelOutcome::Ready(Value::Unit);
        }

        self.heap.close_channel(id);
        if let Some(waiters) = self.channel_waiters.remove(&id) {
            for receiver in waiters.receivers {
                self.wake_channel_process(receiver, id, ChannelResult::Closed);
            }
            for (sender, _) in waiters.senders {
                self.wake_channel_process(sender, id, ChannelResult::Closed);
            }
        }
        ChannelOutcome::Ready(Value::Unit)
    }

    fn park_on_channel(&mut self, id: ChanId) {
        self.io_current = None;
        self.status = ProcStatus::Blocked(BlockReason::Chan(id));
    }

    fn pop_receiver(&mut self, id: ChanId) -> Option<ProcessId> {
        let receiver = self
            .channel_waiters
            .get_mut(&id)
            .and_then(|waiters| waiters.receivers.pop_front());
        self.remove_empty_waiters(id);
        receiver
    }

    fn admit_waiting_sender(&mut self, id: ChanId) {
        let sender = self
            .channel_waiters
            .get_mut(&id)
            .and_then(|waiters| waiters.senders.pop_front());
        self.remove_empty_waiters(id);

        if let Some((process, value)) = sender {
            self.heap.push_channel(id, value);
            self.wake_channel_process(process, id, ChannelResult::Value(Value::Unit));
        }
    }

    fn remove_empty_waiters(&mut self, id: ChanId) {
        let empty = self
            .channel_waiters
            .get(&id)
            .is_some_and(|waiters| waiters.senders.is_empty() && waiters.receivers.is_empty());
        if empty {
            self.channel_waiters.remove(&id);
        }
    }

    fn wake_channel_process(&mut self, process_id: ProcessId, id: ChanId, result: ChannelResult) {
        let process = &mut self.inactive_processes[process_id];
        if process.status != ProcStatus::Blocked(BlockReason::Chan(id)) {
            solveig_base::ice!("channel wakeup did not match the blocked process")
        }
        if process.channel_result.is_some() {
            solveig_base::ice!("channel wakeup overwrote an unconsumed result")
        }
        process.channel_result = Some(result);
        process.status = ProcStatus::Runnable;
        self.run_queue.push_back(process_id);
    }
}

pub(super) fn bounded_channel_capacity(capacity: i64) -> Result<u32, RuntimeError> {
    u32::try_from(capacity)
        .ok()
        .filter(|capacity| *capacity > 0)
        .ok_or_else(|| {
            RuntimeError::defect(DefectKind::Error(format!(
                "newBoundedChan capacity must be between 1 and {}",
                u32::MAX
            )))
        })
}

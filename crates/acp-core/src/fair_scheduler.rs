use std::collections::VecDeque;
use tracing::{info, warn};

const MAX_QUEUE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Normal = 0,
    High = 1,
}

#[derive(Debug, Clone)]
pub struct QueuedItem {
    pub content: String,
    pub from: Option<String>,
    pub reply_to: Option<String>,
    pub priority: Priority,
}

/// Max seconds before a stuck busy state is auto-cleared.
const BUSY_TIMEOUT_SECS: i64 = 300; // 5 minutes

pub struct FairScheduler {
    queue: VecDeque<QueuedItem>,
    busy: bool,
    busy_since: Option<i64>,
    last_sender: Option<String>,
}

impl Default for FairScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl FairScheduler {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            busy: false,
            busy_since: None,
            last_sender: None,
        }
    }

    /// Enqueue a message. Returns Ok(true) if should send immediately, Ok(false) if queued.
    pub fn enqueue(
        &mut self,
        content: &str,
        from: Option<&str>,
        priority: Priority,
    ) -> Result<bool, String> {
        self.enqueue_with_reply(content, from, None, priority)
    }

    pub fn enqueue_with_reply(
        &mut self,
        content: &str,
        from: Option<&str>,
        reply_to: Option<&str>,
        priority: Priority,
    ) -> Result<bool, String> {
        // Auto-recover from stuck busy state
        if self.busy {
            if let Some(since) = self.busy_since {
                let now = chrono::Utc::now().timestamp();
                if now - since > BUSY_TIMEOUT_SECS {
                    warn!(
                        elapsed = now - since,
                        "scheduler busy timeout ({BUSY_TIMEOUT_SECS}s), force-clearing"
                    );
                    self.busy = false;
                    self.busy_since = None;
                }
            }
        }
        if !self.busy {
            self.busy = true;
            self.busy_since = Some(chrono::Utc::now().timestamp());
            self.last_sender = from.map(String::from);
            return Ok(true);
        }
        if self.queue.len() >= MAX_QUEUE {
            warn!("scheduler queue full, dropping message");
            return Err(format!("队列已满（{MAX_QUEUE}），消息被丢弃"));
        }
        info!(from = ?from, "queuing message for main");
        self.queue.push_back(QueuedItem {
            content: content.to_string(),
            from: from.map(String::from),
            reply_to: reply_to.map(String::from),
            priority,
        });
        Ok(false)
    }

    /// Drain the next item: high priority first, then fair round-robin.
    pub fn drain(&mut self) -> Option<QueuedItem> {
        self.busy = false;
        self.busy_since = None;

        let now = chrono::Utc::now().timestamp();

        // 1. High priority first
        if let Some(idx) = self.queue.iter().position(|q| q.priority == Priority::High) {
            self.busy = true;
            self.busy_since = Some(now);
            let item = self.queue.remove(idx).unwrap();
            self.last_sender = item.from.clone();
            return Some(item);
        }

        // 2. Fair: pick a different sender than last
        if let Some(ref last) = self.last_sender {
            if let Some(idx) = self
                .queue
                .iter()
                .position(|q| q.from.as_deref() != Some(last))
            {
                self.busy = true;
                self.busy_since = Some(now);
                let item = self.queue.remove(idx).unwrap();
                self.last_sender = item.from.clone();
                return Some(item);
            }
        }

        // 3. Fallback: first in queue
        if let Some(item) = self.queue.pop_front() {
            self.busy = true;
            self.busy_since = Some(now);
            self.last_sender = item.from.clone();
            return Some(item);
        }

        None
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_when_free() {
        let mut s = FairScheduler::new();
        assert!(s.enqueue("hello", None, Priority::Normal).unwrap());
        assert!(s.is_busy());
    }

    #[test]
    fn queues_when_busy() {
        let mut s = FairScheduler::new();
        assert!(s.enqueue("first", None, Priority::Normal).unwrap());
        assert!(!s.enqueue("second", Some("w1"), Priority::Normal).unwrap());
        assert_eq!(s.queue_depth(), 1);
    }

    #[test]
    fn high_priority_first() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        s.enqueue("low", Some("w1"), Priority::Normal).unwrap();
        s.enqueue("urgent", Some("w2"), Priority::High).unwrap();

        let next = s.drain().unwrap();
        assert_eq!(next.content, "urgent");
        assert_eq!(next.priority, Priority::High);
    }

    #[test]
    fn fair_rotation() {
        let mut s = FairScheduler::new();
        // w1 goes first (immediate)
        assert!(s.enqueue("msg1", Some("w1"), Priority::Normal).unwrap());
        // Queue: w2, w1, w3
        assert!(!s.enqueue("msg2", Some("w2"), Priority::Normal).unwrap());
        assert!(!s.enqueue("msg3", Some("w1"), Priority::Normal).unwrap());
        assert!(!s.enqueue("msg4", Some("w3"), Priority::Normal).unwrap());

        // last_sender = w1, so pick first != w1 → w2
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w2".to_string()));
        // last_sender = w2, pick first != w2 → w1
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w1".to_string()));
        // last_sender = w1, pick first != w1 → w3
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w3".to_string()));
    }

    #[test]
    fn queue_full_rejects() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        for i in 0..MAX_QUEUE {
            s.enqueue(&format!("msg{i}"), Some("w1"), Priority::Normal)
                .unwrap();
        }
        assert!(s.enqueue("overflow", Some("w1"), Priority::Normal).is_err());
    }

    #[test]
    fn preserves_reply_to() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        s.enqueue_with_reply("reply msg", Some("w1"), Some("w1"), Priority::Normal)
            .unwrap();
        let next = s.drain().unwrap();
        assert_eq!(next.reply_to, Some("w1".to_string()));
    }

    #[test]
    fn drain_empty_returns_none() {
        let mut s = FairScheduler::new();
        assert!(s.drain().is_none());
        assert!(!s.is_busy());
    }

    #[test]
    fn is_busy_reflects_state() {
        let mut s = FairScheduler::new();
        assert!(!s.is_busy());
        assert!(s.enqueue("msg", None, Priority::Normal).unwrap());
        assert!(s.is_busy());
        s.drain();
        assert!(!s.is_busy());
    }
}

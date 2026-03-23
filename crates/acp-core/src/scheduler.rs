use std::collections::VecDeque;
use tracing::{info, warn};

const MAX_MAIN_QUEUE: usize = 10;

#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub content: String,
    pub from: Option<String>,
    pub reply_to: Option<String>,
}

/// Scheduler manages the main agent queue (serial) and agent dispatch.
/// The actual prompt sending is done by the caller (Channel/App layer)
/// since it needs access to the client handles.
pub struct Scheduler {
    main_queue: VecDeque<QueuedMessage>,
    main_busy: bool,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            main_queue: VecDeque::new(),
            main_busy: false,
        }
    }

    /// Try to enqueue a message for the main agent.
    /// Returns true if the message should be sent immediately (main is free).
    /// Returns false if it was queued.
    /// Returns Err if the queue is full.
    pub fn push_to_main(&mut self, content: String, from: Option<String>) -> Result<bool, String> {
        self.push_to_main_with_reply(content, from, None)
    }

    /// Push a message to the main queue, preserving reply_to context for bus replies.
    pub fn push_to_main_with_reply(
        &mut self,
        content: String,
        from: Option<String>,
        reply_to: Option<String>,
    ) -> Result<bool, String> {
        if self.main_busy {
            if self.main_queue.len() >= MAX_MAIN_QUEUE {
                warn!("main queue full, dropping message");
                return Err(format!("main 队列已满（{MAX_MAIN_QUEUE}），消息被丢弃"));
            }
            info!(msg_len = content.len(), "queuing message for main");
            self.main_queue
                .push_back(QueuedMessage { content, from, reply_to });
            return Ok(false);
        }
        self.main_busy = true;
        Ok(true)
    }

    /// Called when main finishes processing. Returns next queued message if any.
    pub fn main_done(&mut self) -> Option<QueuedMessage> {
        self.main_busy = false;
        self.main_queue.pop_front()
    }

    pub fn is_main_busy(&self) -> bool {
        self.main_busy
    }

    pub fn main_queue_depth(&self) -> usize {
        self.main_queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_send_when_free() {
        let mut s = Scheduler::new();
        assert!(s.push_to_main("hello".into(), None).unwrap());
    }

    #[test]
    fn queues_when_busy() {
        let mut s = Scheduler::new();
        assert!(s.push_to_main("first".into(), None).unwrap());
        assert!(!s.push_to_main("second".into(), None).unwrap());
        assert_eq!(s.main_queue_depth(), 1);
    }

    #[test]
    fn drains_on_done() {
        let mut s = Scheduler::new();
        s.push_to_main("first".into(), None).unwrap();
        s.push_to_main("second".into(), None).unwrap();
        let next = s.main_done().unwrap();
        assert_eq!(next.content, "second");
    }

    #[test]
    fn rejects_when_full() {
        let mut s = Scheduler::new();
        s.push_to_main("first".into(), None).unwrap();
        for i in 0..10 {
            s.push_to_main(format!("msg {i}"), None).unwrap();
        }
        assert!(s.push_to_main("overflow".into(), None).is_err());
    }

    #[test]
    fn preserves_reply_to_through_queue() {
        let mut s = Scheduler::new();
        // First message makes main busy
        s.push_to_main("first".into(), None).unwrap();
        // Second message with reply_to gets queued
        s.push_to_main_with_reply(
            "bob's report".into(),
            Some("bob".into()),
            Some("bob".into()),
        )
        .unwrap();
        // Drain: reply_to must survive
        let next = s.main_done().unwrap();
        assert_eq!(next.content, "bob's report");
        assert_eq!(next.from, Some("bob".into()));
        assert_eq!(next.reply_to, Some("bob".into()));
    }

    #[test]
    fn preserves_none_reply_to_through_queue() {
        let mut s = Scheduler::new();
        s.push_to_main("first".into(), None).unwrap();
        // Message without reply_to (user prompt)
        s.push_to_main_with_reply("user msg".into(), None, None).unwrap();
        let next = s.main_done().unwrap();
        assert_eq!(next.reply_to, None);
    }
}

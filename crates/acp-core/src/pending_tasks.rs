use std::collections::HashSet;

/// Tracks agents dispatched by the orchestrator (main/main-b).
/// When all tracked agents complete, signals readiness for delivery.
pub struct PendingTasks {
    agents: HashSet<String>,
    /// True once at least one agent has been tracked. Prevents false
    /// "all done" signals when no agents were ever dispatched.
    ever_tracked: bool,
}

impl PendingTasks {
    pub fn new() -> Self {
        Self {
            agents: HashSet::new(),
            ever_tracked: false,
        }
    }

    /// Record that an agent was dispatched for a task.
    pub fn track(&mut self, agent_name: &str) {
        self.agents.insert(agent_name.to_string());
        self.ever_tracked = true;
    }

    /// Mark an agent as completed. Returns true only if at least one agent
    /// was tracked AND all tracked agents are now done.
    pub fn complete(&mut self, agent_name: &str) -> bool {
        self.agents.remove(agent_name);
        self.ever_tracked && self.agents.is_empty()
    }

    /// Check if all dispatched agents have completed.
    pub fn is_all_done(&self) -> bool {
        self.agents.is_empty()
    }

    /// Number of agents still pending.
    pub fn pending_count(&self) -> usize {
        self.agents.len()
    }

    /// Remove an agent without treating it as completion (e.g. removed by user).
    pub fn untrack(&mut self, agent_name: &str) {
        self.agents.remove(agent_name);
    }
}

impl Default for PendingTasks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_all_done() {
        let pt = PendingTasks::new();
        assert!(pt.is_all_done());
    }

    #[test]
    fn track_then_complete_one() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        assert!(!pt.is_all_done());
        assert_eq!(pt.pending_count(), 1);

        let all_done = pt.complete("alice");
        assert!(all_done);
        assert!(pt.is_all_done());
    }

    #[test]
    fn track_multiple_complete_all() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        pt.track("bob");
        pt.track("carol");

        assert!(!pt.complete("alice")); // 2 remaining
        assert!(!pt.complete("bob")); // 1 remaining
        assert!(pt.complete("carol")); // all done
    }

    #[test]
    fn complete_unknown_agent_is_noop() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        let all_done = pt.complete("unknown");
        assert!(!all_done); // alice still pending
    }

    #[test]
    fn untrack_does_not_signal_completion() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        pt.track("bob");
        pt.untrack("alice");
        assert_eq!(pt.pending_count(), 1);
        assert!(!pt.is_all_done());
    }

    #[test]
    fn duplicate_track_is_idempotent() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        pt.track("alice");
        assert_eq!(pt.pending_count(), 1);
        assert!(pt.complete("alice"));
    }

    #[test]
    fn complete_untracked_agent_does_not_fire() {
        // When nothing was ever tracked, complete() should NOT return true
        let mut pt = PendingTasks::new();
        assert!(!pt.complete("unknown"));
    }

    /// Simulates the group discussion scenario from log 074220:
    /// 3 agents tracked, group prompts should NOT call complete() per-round,
    /// only after all rounds finish should complete() be called for each.
    #[test]
    fn group_discussion_completes_only_after_all_rounds() {
        let mut pt = PendingTasks::new();
        pt.track("rust-fan");
        pt.track("go-fan");
        pt.track("judge");

        // Round 1: each agent finishes a group prompt.
        // Caller (do_prompt_inner) should NOT call complete() for group prompts.
        // So pending count stays at 3.
        assert_eq!(pt.pending_count(), 3);

        // Round 2: same — no complete() calls during group prompts.
        assert_eq!(pt.pending_count(), 3);

        // All rounds done — dispatch_group_sequential calls complete() for each.
        assert!(!pt.complete("rust-fan")); // 2 remaining
        assert!(!pt.complete("go-fan")); // 1 remaining
        assert!(pt.complete("judge")); // all done → notify main
    }

    /// Mixed scenario: some agents do independent tasks, others are in a group.
    /// Independent agents complete() normally; group agents complete() only at end.
    #[test]
    fn mixed_independent_and_group_agents() {
        let mut pt = PendingTasks::new();
        pt.track("researcher"); // independent task
        pt.track("debater-a"); // group discussion
        pt.track("debater-b"); // group discussion

        // researcher finishes its independent task
        assert!(!pt.complete("researcher")); // 2 still pending (group agents)

        // group discussion rounds happen — no complete() calls
        assert_eq!(pt.pending_count(), 2);

        // group discussion ends
        assert!(!pt.complete("debater-a")); // 1 remaining
        assert!(pt.complete("debater-b")); // all done
    }

    /// Completing the same agent multiple times (e.g. if called redundantly)
    /// should not cause issues. Second complete() is a no-op.
    #[test]
    fn double_complete_is_safe() {
        let mut pt = PendingTasks::new();
        pt.track("alice");
        pt.track("bob");

        assert!(!pt.complete("alice"));
        // alice already completed — second call is no-op, bob still pending
        assert!(!pt.complete("alice"));
        assert_eq!(pt.pending_count(), 1);
        assert!(pt.complete("bob"));
    }
}

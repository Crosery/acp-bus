use std::collections::HashMap;

/// Directed wait-for graph for deadlock detection.
/// Each edge means "agent X is waiting for agent Y to reply."
pub struct WaitGraph {
    // agent_name → agent_name_it_waits_for
    edges: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct DeadlockError {
    pub cycle: Vec<String>,
}

impl std::fmt::Display for DeadlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deadlock detected: {}", self.cycle.join(" → "))
    }
}

impl std::error::Error for DeadlockError {}

impl WaitGraph {
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    /// Add a wait edge: `from` is waiting for `to`.
    /// Returns Err if adding this edge would create a cycle (deadlock).
    pub fn add_wait(&mut self, from: &str, to: &str) -> Result<(), DeadlockError> {
        // Self-wait is an immediate deadlock
        if from == to {
            return Err(DeadlockError {
                cycle: vec![from.to_string(), to.to_string()],
            });
        }

        if self.would_cycle(from, to) {
            let cycle = self.build_cycle_path(from, to);
            return Err(DeadlockError { cycle });
        }

        self.edges.insert(from.to_string(), to.to_string());
        Ok(())
    }

    /// Remove wait edge for agent (when reply is received or timeout).
    pub fn remove_wait(&mut self, from: &str) {
        self.edges.remove(from);
    }

    /// Check if adding edge from→to would create a cycle.
    /// Cycle detection: follow the chain to→?→?→... and check if it reaches `from`.
    fn would_cycle(&self, from: &str, to: &str) -> bool {
        let mut current = to;
        loop {
            match self.edges.get(current) {
                Some(next) => {
                    if next == from {
                        return true;
                    }
                    current = next.as_str();
                }
                None => return false,
            }
        }
    }

    /// Build the full cycle path for error reporting: [from, to, ..., from].
    fn build_cycle_path(&self, from: &str, to: &str) -> Vec<String> {
        let mut path = vec![from.to_string()];
        let mut current = to;
        loop {
            path.push(current.to_string());
            if current == from {
                break;
            }
            match self.edges.get(current) {
                Some(next) => current = next.as_str(),
                None => break,
            }
        }
        path
    }

    /// Clean up all edges involving a specific agent (when agent exits).
    pub fn cleanup_agent(&mut self, name: &str) {
        // Remove edges where `name` is the one waiting
        self.edges.remove(name);
        // Remove edges where `name` is the one being waited for
        self.edges.retain(|_, v| v != name);
    }

    /// Get who an agent is waiting for (for status display).
    pub fn waiting_for(&self, name: &str) -> Option<&str> {
        self.edges.get(name).map(|s| s.as_str())
    }
}

impl Default for WaitGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_wait_simple() {
        let mut g = WaitGraph::new();
        assert!(g.add_wait("alice", "bob").is_ok());
        assert_eq!(g.waiting_for("alice"), Some("bob"));
        assert_eq!(g.waiting_for("bob"), None);
    }

    #[test]
    fn detect_direct_cycle() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        let err = g.add_wait("bob", "alice").unwrap_err();
        assert_eq!(err.cycle, vec!["bob", "alice", "bob"]);
    }

    #[test]
    fn detect_indirect_cycle() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        g.add_wait("bob", "carol").unwrap();
        let err = g.add_wait("carol", "alice").unwrap_err();
        assert_eq!(err.cycle, vec!["carol", "alice", "bob", "carol"]);
    }

    #[test]
    fn no_false_positive() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        // carol→bob doesn't create cycle (carol isn't in alice's chain)
        assert!(g.add_wait("carol", "bob").is_ok());
    }

    #[test]
    fn remove_wait_breaks_cycle_potential() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        g.remove_wait("alice");
        // Now bob→alice should be fine
        assert!(g.add_wait("bob", "alice").is_ok());
    }

    #[test]
    fn cleanup_agent_removes_all_edges() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        g.add_wait("carol", "bob").unwrap();
        g.cleanup_agent("bob");
        // alice and carol should no longer be waiting
        assert_eq!(g.waiting_for("alice"), None);
        assert_eq!(g.waiting_for("carol"), None);
    }

    #[test]
    fn self_wait_is_deadlock() {
        let mut g = WaitGraph::new();
        let err = g.add_wait("alice", "alice").unwrap_err();
        assert_eq!(err.cycle, vec!["alice", "alice"]);
    }

    #[test]
    fn replace_existing_wait() {
        let mut g = WaitGraph::new();
        g.add_wait("alice", "bob").unwrap();
        // alice now waits for carol instead
        assert!(g.add_wait("alice", "carol").is_ok());
        assert_eq!(g.waiting_for("alice"), Some("carol"));
    }
}

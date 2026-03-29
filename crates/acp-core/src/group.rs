use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub name: String,
    pub creator: String,
    pub members: HashSet<String>,
    pub created_at: i64,
}

impl Group {
    pub fn new(name: &str, creator: &str) -> Self {
        Self {
            name: name.to_string(),
            creator: creator.to_string(),
            members: HashSet::new(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn add_member(&mut self, name: &str) -> bool {
        self.members.insert(name.to_string())
    }

    pub fn remove_member(&mut self, name: &str) -> bool {
        if name == self.creator {
            return false;
        }
        self.members.remove(name)
    }

    pub fn is_member(&self, name: &str) -> bool {
        self.members.contains(name)
    }

    pub fn other_members(&self, exclude: &str) -> Vec<&String> {
        self.members
            .iter()
            .filter(|m| m.as_str() != exclude)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_group() {
        let g = Group::new("research", "main");
        assert_eq!(g.name, "research");
        assert!(!g.members.contains("main")); // creator is NOT a member
        assert_eq!(g.creator, "main");
    }

    #[test]
    fn add_and_remove_member() {
        let mut g = Group::new("team", "main");
        assert!(g.add_member("w1"));
        assert!(g.is_member("w1"));
        assert!(g.remove_member("w1"));
        assert!(!g.is_member("w1"));
    }

    #[test]
    fn cannot_remove_creator() {
        let mut g = Group::new("team", "main");
        g.add_member("main"); // explicitly add creator
        assert!(!g.remove_member("main"));
        assert!(g.is_member("main"));
    }

    #[test]
    fn other_members_excludes_self() {
        let mut g = Group::new("team", "main");
        g.add_member("w1");
        g.add_member("w2");
        let others = g.other_members("w1");
        assert!(others.contains(&&"w2".to_string()));
        assert!(!others.contains(&&"w1".to_string()));
    }

    #[test]
    fn add_duplicate_returns_false() {
        let mut g = Group::new("team", "main");
        g.add_member("w1");
        assert!(!g.add_member("w1"));
    }

    #[test]
    fn creator_not_auto_member() {
        // Creator should NOT be auto-added as member.
        // They can observe via bus_list_agents / group_history,
        // but should not be prompted during group discussions.
        let g = Group::new("debate", "main");
        assert!(
            !g.is_member("main"),
            "creator should not be auto-added as member"
        );
        assert_eq!(g.creator, "main");
    }
}

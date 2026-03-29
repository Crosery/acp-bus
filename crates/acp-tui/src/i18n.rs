//! Centralized English UI strings for the TUI.
//!
//! All user-facing text lives here so that:
//! 1. Strings are easy to find and update in one place.
//! 2. A future locale system only needs to swap this module.

// === Agent Status Labels ===

pub const STATUS_IDLE: &str = "Idle";
pub const STATUS_THINKING: &str = "Thinking";
pub const STATUS_TYPING: &str = "Typing";
pub const STATUS_READY: &str = "Ready";
pub const STATUS_CONNECTING: &str = "Connecting";
pub const STATUS_ERROR: &str = "Error";
pub const STATUS_DISCONNECTED: &str = "Disconnected";

pub fn status_waiting(target: &str) -> String {
    format!("Waiting {target}")
}

// === Sidebar Tab Labels ===

pub const TAB_DM: &str = " DM ";
pub const TAB_GROUPS: &str = " Groups ";

// === Sidebar Hints ===

pub const SIDEBAR_HINTS: &[(&str, &str)] = &[
    ("^C", "Quit"),
    ("^Q", "Cancel"),
    ("^B", "Sidebar"),
    ("Tab", "DM/Groups"),
    ("^N/P", "Switch"),
    ("^J/K", "Scroll"),
    ("^D/U", "Page"),
    ("Enter", "Send"),
    ("^Enter", "Newline"),
    ("^V", "Paste img"),
];

// === Sidebar Groups ===

pub const NO_GROUPS: &str = "No groups";
pub const NO_GROUPS_HINT: &str = "/group create";

// === Input Placeholders ===

pub fn placeholder_agent(name: &str) -> String {
    format!("Send to {name}... (Enter send, @agent route, /help)")
}

pub const PLACEHOLDER_SYSTEM: &str = "Type a message... (Tab complete, @agent route, /add /help)";

// === Input Status Labels (reused in input box) ===

pub const INPUT_STATUS_THINKING: &str = "Thinking";
pub const INPUT_STATUS_TYPING: &str = "Typing";
pub const INPUT_STATUS_CONNECTING: &str = "Connecting";
pub const INPUT_STATUS_ERROR: &str = "Error";
pub const INPUT_STATUS_IDLE: &str = "Idle";

// === Command Descriptions (autocomplete popup) ===

pub const CMD_ADD: &str = "Add agent";
pub const CMD_REMOVE: &str = "Remove agent";
pub const CMD_LIST: &str = "List agents";
pub const CMD_ADAPTERS: &str = "Adapters";
pub const CMD_CANCEL: &str = "Cancel task";
pub const CMD_GROUP: &str = "Group mgmt";
pub const CMD_SAVE: &str = "Save snapshot";
pub const CMD_HELP: &str = "Help";
pub const CMD_QUIT: &str = "Quit";

// === System Messages (lifecycle) ===

pub fn sys_adapter_error(err: &str) -> String {
    format!("adapter error: {err}")
}

pub fn sys_connecting(name: &str) -> String {
    format!("{name} connecting...")
}

pub fn sys_online(name: &str, adapter: &str) -> String {
    format!("{name} ({adapter}) online")
}

pub fn sys_connect_failed(name: &str, err: &str) -> String {
    format!("{name} connection failed: {err}")
}

pub fn sys_abnormal_exit(name: &str, code: Option<i32>) -> String {
    format!("{name} abnormal exit (code={code:?})")
}

pub fn sys_normal_exit(name: &str, code: Option<i32>) -> String {
    format!("{name} exited (code={code:?})")
}

pub const SYS_SENDER_SYSTEM: &str = "System";

// === System Messages (prompting / queue) ===

pub fn sys_agent_complete(name: &str) -> String {
    format!("{name} completed")
}

pub fn sys_agent_error(name: &str, err: &str) -> String {
    format!("{name} error: {err}")
}

pub fn sys_all_main_busy(max: usize, depth: usize) -> String {
    format!(
        "All main instances busy ({max}/{max}), message queued (queue: {depth})"
    )
}

pub fn sys_agent_not_connected(name: &str) -> String {
    format!("{name} not connected (timeout), task saved")
}

pub fn sys_image_pasted(path: &str) -> String {
    format!("[User pasted an image, use Read tool to view: {path}]")
}

pub fn sys_image_pasted_with_text(path: &str) -> String {
    format!("[User pasted an image: {path}]")
}

// === Group Messages (bus_events) ===

pub fn err_group_exists(name: &str) -> String {
    format!("group {name} already exists")
}

pub const ERR_GROUP_NO_MEMBERS: &str = "group not found or no other members";

pub fn err_group_not_found(name: &str) -> String {
    format!("group {name} not found")
}

pub fn err_agent_not_found(name: &str) -> String {
    format!("agent {name} not found")
}

pub fn sys_member_joined_group(member: &str, group: &str) -> String {
    format!("{member} joined group [{group}]")
}

pub fn err_member_already_in_group(member: &str) -> String {
    format!("{member} already in group")
}

// === Group Display (status bar format) ===

pub fn group_label(name: &str) -> String {
    format!("Group {name}")
}

// === Command Messages ===

pub const CMD_ADD_USAGE: &str =
    "Usage: /add <name> <adapter>\nAvailable adapters: claude, c1, c2, gemini, codex";
pub const CMD_REMOVE_USAGE: &str = "Usage: /remove <name>";
pub const CMD_CANCEL_USAGE: &str = "Usage: /cancel <name>";
pub const CMD_CANNOT_REMOVE_MAIN: &str = "Cannot remove main agent";
pub const CMD_NO_AGENTS: &str = "No agents";

pub fn cmd_cancelled(name: &str) -> String {
    format!("Cancelled {name}")
}

pub fn cmd_not_found(name: &str) -> String {
    format!("{name} not found")
}

pub fn cmd_saved(path: &str) -> String {
    format!("Saved: {path}")
}

pub fn cmd_save_failed(err: &str) -> String {
    format!("Save failed: {err}")
}

pub const CMD_HELP_TEXT: &str = "\
/add <name> <adapter>  Add agent\n\
/remove <name>         Remove agent\n\
/list                  List agents\n\
/adapters              List adapters\n\
/cancel <name>         Cancel task\n\
/save                  Save channel snapshot\n\
/quit                  Quit\n\
Use @name in message to route to agent\n\
Messages without @mention go to main\n\
Tab to complete commands and @agent";

pub fn cmd_unknown(cmd: &str) -> String {
    format!("Unknown command: {cmd}, /help for help")
}

pub const CMD_GROUP_USAGE: &str = "\
/group create <name> <member1> <member2> ...\n\
/group add <name> <member>\n\
/group list\n\
/group remove <name> <member>";
pub const CMD_GROUP_CREATE_USAGE: &str = "Usage: /group create <name> <members...>";
pub const CMD_GROUP_ADD_USAGE: &str = "Usage: /group add <name> <member>";
pub const CMD_GROUP_REMOVE_USAGE: &str = "Usage: /group remove <name> <member>";
pub const CMD_NO_GROUPS: &str = "No groups";
pub const CMD_GROUP_UNKNOWN_SUB: &str = "Unknown subcommand, use /group for help";

pub fn cmd_group_created(name: &str, members: &str) -> String {
    format!("Group [{name}] members: {members}")
}

pub fn cmd_group_exists(name: &str) -> String {
    format!("Group {name} already exists")
}

pub fn cmd_member_joined(member: &str, group: &str) -> String {
    format!("{member} joined group [{group}]")
}

pub fn cmd_group_not_found(name: &str) -> String {
    format!("Group {name} not found")
}

pub fn cmd_member_left(member: &str, group: &str) -> String {
    format!("{member} left group [{group}]")
}

pub fn cmd_cannot_remove_creator(member: &str) -> String {
    format!("Cannot remove {member} (creator cannot leave)")
}

// === Cancel / Interrupt Messages (mod.rs) ===

pub fn sys_interrupted_agents(names: &str) -> String {
    format!("Interrupted: {names}")
}

pub fn sys_interrupted(name: &str) -> String {
    format!("Interrupted {name}")
}

pub const SYS_CLIPBOARD_NO_IMAGE: &str = "No image found in clipboard";

#[cfg(test)]
mod tests {
    use super::*;

    // --- Agent Status Labels ---

    #[test]
    fn status_constants_not_empty() {
        assert!(!STATUS_IDLE.is_empty());
        assert!(!STATUS_THINKING.is_empty());
        assert!(!STATUS_TYPING.is_empty());
        assert!(!STATUS_READY.is_empty());
        assert!(!STATUS_CONNECTING.is_empty());
        assert!(!STATUS_ERROR.is_empty());
        assert!(!STATUS_DISCONNECTED.is_empty());
    }

    #[test]
    fn status_waiting_includes_target() {
        let result = status_waiting("bob");
        assert!(result.contains("bob"));
        assert!(result.contains("Waiting"));
    }

    // --- Sidebar ---

    #[test]
    fn sidebar_tabs_not_empty() {
        assert!(!TAB_DM.is_empty());
        assert!(!TAB_GROUPS.is_empty());
    }

    #[test]
    fn sidebar_hints_has_entries() {
        assert!(SIDEBAR_HINTS.len() >= 8);
        for (key, desc) in SIDEBAR_HINTS {
            assert!(!key.is_empty());
            assert!(!desc.is_empty());
        }
    }

    #[test]
    fn no_groups_text() {
        assert!(!NO_GROUPS.is_empty());
        assert!(!NO_GROUPS_HINT.is_empty());
    }

    // --- Input Placeholders ---

    #[test]
    fn placeholder_agent_includes_name() {
        let result = placeholder_agent("w1");
        assert!(result.contains("w1"));
    }

    #[test]
    fn placeholder_system_not_empty() {
        assert!(!PLACEHOLDER_SYSTEM.is_empty());
    }

    // --- Input Status Labels ---

    #[test]
    fn input_status_labels_not_empty() {
        assert!(!INPUT_STATUS_THINKING.is_empty());
        assert!(!INPUT_STATUS_TYPING.is_empty());
        assert!(!INPUT_STATUS_CONNECTING.is_empty());
        assert!(!INPUT_STATUS_ERROR.is_empty());
        assert!(!INPUT_STATUS_IDLE.is_empty());
    }

    // --- Command Descriptions ---

    #[test]
    fn command_descriptions_not_empty() {
        assert!(!CMD_ADD.is_empty());
        assert!(!CMD_REMOVE.is_empty());
        assert!(!CMD_LIST.is_empty());
        assert!(!CMD_ADAPTERS.is_empty());
        assert!(!CMD_CANCEL.is_empty());
        assert!(!CMD_GROUP.is_empty());
        assert!(!CMD_SAVE.is_empty());
        assert!(!CMD_HELP.is_empty());
        assert!(!CMD_QUIT.is_empty());
    }

    // --- System Messages (lifecycle) ---

    #[test]
    fn sys_adapter_error_includes_msg() {
        let result = sys_adapter_error("not found");
        assert!(result.contains("not found"));
    }

    #[test]
    fn sys_connecting_includes_name() {
        let result = sys_connecting("main");
        assert!(result.contains("main"));
        assert!(result.contains("connecting"));
    }

    #[test]
    fn sys_online_includes_name_and_adapter() {
        let result = sys_online("main-2", "claude");
        assert!(result.contains("main-2"));
        assert!(result.contains("claude"));
        assert!(result.contains("online"));
    }

    #[test]
    fn sys_connect_failed_includes_details() {
        let result = sys_connect_failed("w1", "timeout");
        assert!(result.contains("w1"));
        assert!(result.contains("timeout"));
    }

    #[test]
    fn sys_exit_messages() {
        let abnormal = sys_abnormal_exit("w1", Some(1));
        assert!(abnormal.contains("w1"));
        assert!(abnormal.contains("abnormal"));

        let normal = sys_normal_exit("main", Some(0));
        assert!(normal.contains("main"));
        assert!(normal.contains("exited"));
    }

    // --- System Messages (prompting) ---

    #[test]
    fn sys_agent_complete_includes_name() {
        let result = sys_agent_complete("worker");
        assert!(result.contains("worker"));
        assert!(result.contains("completed"));
    }

    #[test]
    fn sys_agent_error_includes_details() {
        let result = sys_agent_error("w1", "timeout");
        assert!(result.contains("w1"));
        assert!(result.contains("timeout"));
    }

    #[test]
    fn sys_all_main_busy_format() {
        let result = sys_all_main_busy(5, 3);
        assert!(result.contains("5/5"));
        assert!(result.contains("3"));
        assert!(result.contains("queue"));
    }

    #[test]
    fn sys_agent_not_connected_includes_name() {
        let result = sys_agent_not_connected("main");
        assert!(result.contains("main"));
        assert!(result.contains("not connected"));
    }

    // --- Group Messages ---

    #[test]
    fn err_group_exists_includes_name() {
        let result = err_group_exists("dev-team");
        assert!(result.contains("dev-team"));
        assert!(result.contains("already exists"));
    }

    #[test]
    fn err_group_no_members_not_empty() {
        assert!(!ERR_GROUP_NO_MEMBERS.is_empty());
    }

    #[test]
    fn err_group_not_found_includes_name() {
        let result = err_group_not_found("team");
        assert!(result.contains("team"));
    }

    #[test]
    fn err_agent_not_found_includes_name() {
        let result = err_agent_not_found("w1");
        assert!(result.contains("w1"));
    }

    #[test]
    fn sys_member_joined_group_format() {
        let result = sys_member_joined_group("bob", "team");
        assert!(result.contains("bob"));
        assert!(result.contains("team"));
        assert!(result.contains("joined"));
    }

    #[test]
    fn err_member_already_in_group_includes_name() {
        let result = err_member_already_in_group("alice");
        assert!(result.contains("alice"));
        assert!(result.contains("already"));
    }

    // --- Group Display ---

    #[test]
    fn group_label_includes_name() {
        let result = group_label("dev");
        assert!(result.contains("Group"));
        assert!(result.contains("dev"));
    }

    // --- Command Messages ---

    #[test]
    fn cmd_usage_strings_not_empty() {
        assert!(!CMD_ADD_USAGE.is_empty());
        assert!(!CMD_REMOVE_USAGE.is_empty());
        assert!(!CMD_CANCEL_USAGE.is_empty());
        assert!(!CMD_CANNOT_REMOVE_MAIN.is_empty());
        assert!(!CMD_NO_AGENTS.is_empty());
        assert!(!CMD_HELP_TEXT.is_empty());
        assert!(!CMD_GROUP_USAGE.is_empty());
        assert!(!CMD_GROUP_CREATE_USAGE.is_empty());
        assert!(!CMD_GROUP_ADD_USAGE.is_empty());
        assert!(!CMD_GROUP_REMOVE_USAGE.is_empty());
        assert!(!CMD_NO_GROUPS.is_empty());
        assert!(!CMD_GROUP_UNKNOWN_SUB.is_empty());
    }

    #[test]
    fn cmd_format_functions() {
        assert!(cmd_cancelled("w1").contains("w1"));
        assert!(cmd_not_found("w1").contains("w1"));
        assert!(cmd_saved("/tmp/test").contains("/tmp/test"));
        assert!(cmd_save_failed("io error").contains("io error"));
        assert!(cmd_unknown("/foo").contains("/foo"));
        assert!(cmd_group_created("team", "a, b").contains("team"));
        assert!(cmd_group_exists("team").contains("already exists"));
        assert!(cmd_member_joined("bob", "team").contains("bob"));
        assert!(cmd_group_not_found("team").contains("not found"));
        assert!(cmd_member_left("bob", "team").contains("left"));
        assert!(cmd_cannot_remove_creator("alice").contains("creator"));
    }

    // --- Cancel / Interrupt ---

    #[test]
    fn sys_interrupted_messages() {
        let multi = sys_interrupted_agents("w1, w2");
        assert!(multi.contains("w1, w2"));
        assert!(multi.contains("Interrupted"));

        let single = sys_interrupted("main");
        assert!(single.contains("main"));
        assert!(single.contains("Interrupted"));
    }

    #[test]
    fn clipboard_no_image_not_empty() {
        assert!(!SYS_CLIPBOARD_NO_IMAGE.is_empty());
    }

    // --- Image paste messages ---

    #[test]
    fn sys_image_pasted_messages() {
        let result = sys_image_pasted("/tmp/img.png");
        assert!(result.contains("/tmp/img.png"));
        assert!(result.contains("Read"));

        let result = sys_image_pasted_with_text("/tmp/img.png");
        assert!(result.contains("/tmp/img.png"));
    }
}

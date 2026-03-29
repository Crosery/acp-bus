//! Centralized UI strings for the TUI.
//!
//! All user-facing text lives here so that:
//! 1. Strings are easy to find and update in one place.
//! 2. A future locale system only needs to swap this module.
//!
//! NOTE: UI strings are in Chinese. Agent prompts (sent to LLM) are in English.

// === Agent Status Labels ===

pub const STATUS_IDLE: &str = "空闲";
pub const STATUS_THINKING: &str = "思考中";
pub const STATUS_TYPING: &str = "输出中";
pub const STATUS_READY: &str = "就绪";
pub const STATUS_CONNECTING: &str = "连接中";
pub const STATUS_ERROR: &str = "错误";
pub const STATUS_DISCONNECTED: &str = "断开";

pub fn status_waiting(target: &str) -> String {
    format!("等待 {target}")
}

// === Sidebar Tab Labels ===

pub const TAB_DM: &str = " 私聊 ";
pub const TAB_GROUPS: &str = " 群组 ";

// === Sidebar Hints ===

pub const SIDEBAR_HINTS: &[(&str, &str)] = &[
    ("^C", "退出"),
    ("^Q", "取消任务"),
    ("^B", "收起侧栏"),
    ("Tab", "私聊/群组"),
    ("^N/P", "切换/选择"),
    ("^J/K", "滚动"),
    ("^D/U", "翻页"),
    ("Enter", "发送/确认"),
    ("^Enter", "换行"),
    ("^V", "粘贴图片"),
];

// === Sidebar Groups ===

pub const NO_GROUPS: &str = "无群组";
pub const NO_GROUPS_HINT: &str = "/group create";

// === Input Placeholders ===

pub fn placeholder_agent(name: &str) -> String {
    format!("发送给 {name}… (Enter 发送, @agent 路由, /help)")
}

pub const PLACEHOLDER_SYSTEM: &str = "输入消息… (@agent 路由, /add /help)";

// === Input Status Labels (reused in input box) ===

pub const INPUT_STATUS_THINKING: &str = "思考中";
pub const INPUT_STATUS_TYPING: &str = "输出中";
pub const INPUT_STATUS_CONNECTING: &str = "连接中";
pub const INPUT_STATUS_ERROR: &str = "错误";
pub const INPUT_STATUS_IDLE: &str = "空闲";

// === Command Descriptions (autocomplete popup) ===

pub const CMD_ADD: &str = "添加 agent";
pub const CMD_REMOVE: &str = "移除 agent";
pub const CMD_LIST: &str = "列出 agents";
pub const CMD_ADAPTERS: &str = "可用 adapters";
pub const CMD_CANCEL: &str = "取消任务";
pub const CMD_GROUP: &str = "群组管理";
pub const CMD_SAVE: &str = "保存快照";
pub const CMD_HELP: &str = "帮助";
pub const CMD_QUIT: &str = "退出";

// === System Messages (lifecycle) ===

pub fn sys_adapter_error(err: &str) -> String {
    format!("adapter 错误: {err}")
}

pub fn sys_connecting(name: &str) -> String {
    format!("{name} 正在连接…")
}

pub fn sys_online(name: &str, adapter: &str) -> String {
    format!("{name} ({adapter}) 已上线")
}

pub fn sys_connect_failed(name: &str, err: &str) -> String {
    format!("{name} 连接失败: {err}")
}

pub fn sys_abnormal_exit(name: &str, code: Option<i32>) -> String {
    format!("{name} 异常退出 (code={code:?})")
}

pub fn sys_normal_exit(name: &str, code: Option<i32>) -> String {
    format!("{name} 退出 (code={code:?})")
}

pub const SYS_SENDER_SYSTEM: &str = "系统";

// === System Messages (prompting / queue) ===

pub fn sys_agent_complete(name: &str) -> String {
    format!("{name} 已完成")
}

pub fn sys_agent_error(name: &str, err: &str) -> String {
    format!("{name} 出错: {err}")
}

pub fn sys_all_main_busy(max: usize, depth: usize) -> String {
    format!(
        "所有 main 实例忙碌中（{max}/{max}），消息已排队（队列: {depth}）"
    )
}

pub fn sys_agent_not_connected(name: &str) -> String {
    format!("{name} 未连接（等待超时），任务已暂存")
}

pub fn sys_image_pasted(path: &str) -> String {
    format!("[User pasted an image, use Read tool to view: {path}]")
}

pub fn sys_image_pasted_with_text(path: &str) -> String {
    format!("[User pasted an image: {path}]")
}

// === Group Messages (bus_events) ===

pub fn err_group_exists(name: &str) -> String {
    format!("群组 {name} 已存在")
}

pub const ERR_GROUP_NO_MEMBERS: &str = "群组不存在或无其他成员";

pub fn err_group_not_found(name: &str) -> String {
    format!("群组 {name} 不存在")
}

pub fn err_agent_not_found(name: &str) -> String {
    format!("agent {name} 不存在")
}

pub fn sys_member_joined_group(member: &str, group: &str) -> String {
    format!("{member} 已加入群组 [{group}]")
}

pub fn err_member_already_in_group(member: &str) -> String {
    format!("{member} 已在群组中")
}

// === Group Display (status bar format) ===

pub fn group_label(name: &str) -> String {
    format!("群组 {name}")
}

// === Command Messages ===

pub const CMD_ADD_USAGE: &str =
    "用法: /add <name> <adapter>\n可用 adapters: claude, c1, c2, gemini, codex";
pub const CMD_REMOVE_USAGE: &str = "用法: /remove <name>";
pub const CMD_CANCEL_USAGE: &str = "用法: /cancel <name>";
pub const CMD_CANNOT_REMOVE_MAIN: &str = "无法移除 main agent";
pub const CMD_NO_AGENTS: &str = "无 agents";

pub fn cmd_cancelled(name: &str) -> String {
    format!("已取消 {name}")
}

pub fn cmd_not_found(name: &str) -> String {
    format!("{name} 未找到")
}

pub fn cmd_saved(path: &str) -> String {
    format!("已保存: {path}")
}

pub fn cmd_save_failed(err: &str) -> String {
    format!("保存失败: {err}")
}

pub const CMD_HELP_TEXT: &str = "\
/add <name> <adapter>  添加 agent\n\
/remove <name>         移除 agent\n\
/list                  列出 agents\n\
/adapters              列出可用 adapters\n\
/cancel <name>         取消当前任务\n\
/save                  保存频道快照\n\
/quit                  退出\n\
消息中用 @name 路由到指定 agent\n\
无 @mention 的消息默认发给 main";

pub fn cmd_unknown(cmd: &str) -> String {
    format!("未知命令: {cmd}，输入 /help 查看帮助")
}

pub const CMD_GROUP_USAGE: &str = "\
/group create <name> <member1> <member2> ...\n\
/group add <name> <member>\n\
/group list\n\
/group remove <name> <member>";
pub const CMD_GROUP_CREATE_USAGE: &str = "用法: /group create <name> <members...>";
pub const CMD_GROUP_ADD_USAGE: &str = "用法: /group add <name> <member>";
pub const CMD_GROUP_REMOVE_USAGE: &str = "用法: /group remove <name> <member>";
pub const CMD_NO_GROUPS: &str = "无群组";
pub const CMD_GROUP_UNKNOWN_SUB: &str = "未知子命令，输入 /group 查看帮助";

pub fn cmd_group_created(name: &str, members: &str) -> String {
    format!("群组 [{name}] 成员: {members}")
}

pub fn cmd_group_exists(name: &str) -> String {
    format!("群组 {name} 已存在")
}

pub fn cmd_member_joined(member: &str, group: &str) -> String {
    format!("{member} 已加入群组 [{group}]")
}

pub fn cmd_group_not_found(name: &str) -> String {
    format!("群组 {name} 不存在")
}

pub fn cmd_member_left(member: &str, group: &str) -> String {
    format!("{member} 已退出群组 [{group}]")
}

pub fn cmd_cannot_remove_creator(member: &str) -> String {
    format!("无法移除 {member}（创建者不能退出）")
}

// === Cancel / Interrupt Messages (mod.rs) ===

pub fn sys_interrupted_agents(names: &str) -> String {
    format!("已中断: {names}")
}

pub fn sys_interrupted(name: &str) -> String {
    format!("已中断 {name}")
}

pub const SYS_CLIPBOARD_NO_IMAGE: &str = "剪贴板中未找到图片";

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
        assert!(result.contains("等待"));
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
        assert!(result.contains("连接"));
    }

    #[test]
    fn sys_online_includes_name_and_adapter() {
        let result = sys_online("main-2", "claude");
        assert!(result.contains("main-2"));
        assert!(result.contains("claude"));
        assert!(result.contains("上线"));
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
        assert!(abnormal.contains("异常"));

        let normal = sys_normal_exit("main", Some(0));
        assert!(normal.contains("main"));
        assert!(normal.contains("退出"));
    }

    // --- System Messages (prompting) ---

    #[test]
    fn sys_agent_complete_includes_name() {
        let result = sys_agent_complete("worker");
        assert!(result.contains("worker"));
        assert!(result.contains("已完成"));
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
        assert!(result.contains("队列"));
    }

    #[test]
    fn sys_agent_not_connected_includes_name() {
        let result = sys_agent_not_connected("main");
        assert!(result.contains("main"));
        assert!(result.contains("未连接"));
    }

    // --- Group Messages ---

    #[test]
    fn err_group_exists_includes_name() {
        let result = err_group_exists("dev-team");
        assert!(result.contains("dev-team"));
        assert!(result.contains("已存在"));
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
        assert!(result.contains("加入"));
    }

    #[test]
    fn err_member_already_in_group_includes_name() {
        let result = err_member_already_in_group("alice");
        assert!(result.contains("alice"));
        assert!(result.contains("已在"));
    }

    // --- Group Display ---

    #[test]
    fn group_label_includes_name() {
        let result = group_label("dev");
        assert!(result.contains("群组"));
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
        assert!(cmd_group_exists("team").contains("已存在"));
        assert!(cmd_member_joined("bob", "team").contains("bob"));
        assert!(cmd_group_not_found("team").contains("不存在"));
        assert!(cmd_member_left("bob", "team").contains("退出"));
        assert!(cmd_cannot_remove_creator("alice").contains("创建者"));
    }

    // --- Cancel / Interrupt ---

    #[test]
    fn sys_interrupted_messages() {
        let multi = sys_interrupted_agents("w1, w2");
        assert!(multi.contains("w1, w2"));
        assert!(multi.contains("中断"));

        let single = sys_interrupted("main");
        assert!(single.contains("main"));
        assert!(single.contains("中断"));
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

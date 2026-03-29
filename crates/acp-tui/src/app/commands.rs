use tokio::sync::mpsc;

use acp_core::channel::{MessageKind, MessageStatus, MessageTransport};
use acp_core::client::BusEvent;

use crate::i18n;

use super::{do_prompt, start_agent_bg, BusContext};

/// Result of executing a slash command.
#[allow(dead_code)]
pub(crate) enum CommandResult {
    /// Command succeeded normally.
    Ok,
    /// User requested quit.
    Quit,
    /// Command failed with an error message (already posted to channel).
    Error(String),
}

/// Handle all TUI slash commands, delegating agent spawning to `start_agent_bg`.
pub(crate) async fn handle_command(
    text: &str,
    ctx: &BusContext,
    bus_tx: &mpsc::UnboundedSender<BusEvent>,
    _cwd: &str,
    _default_adapter: &str,
) -> CommandResult {
    let parts: Vec<&str> = text.splitn(4, ' ').collect();
    let cmd = parts[0];

    match cmd {
        "/add" => {
            if parts.len() < 3 {
                let mut ch = ctx.channel.lock().await;
                ch.post(
                    i18n::SYS_SENDER_SYSTEM,
                    i18n::CMD_ADD_USAGE,
                    true,
                );
                return CommandResult::Ok;
            }
            let name = parts[1].to_string();
            let adapter_name = parts[2].to_string();
            let task = if parts.len() >= 4 {
                Some(parts[3].to_string())
            } else {
                None
            };

            start_agent_bg(
                name.clone(),
                adapter_name,
                ctx.clone(),
                Some(bus_tx.clone()),
            )
            .await;

            if let Some(task) = task {
                // Post dispatch message immediately so it appears before
                // any bus messages that arrive while the agent connects.
                {
                    let mut ch = ctx.channel.lock().await;
                    ch.post_directed(
                        "main",
                        &name,
                        &task,
                        MessageKind::Task,
                        MessageTransport::MentionRoute,
                        MessageStatus::Delivered,
                    );
                }
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    do_prompt(name, task, ctx).await;
                });
            }
            CommandResult::Ok
        }
        "/remove" | "/rm" => {
            if parts.len() < 2 {
                let mut ch = ctx.channel.lock().await;
                ch.post(i18n::SYS_SENDER_SYSTEM, i18n::CMD_REMOVE_USAGE, true);
                return CommandResult::Ok;
            }
            let name = parts[1].to_string();
            if name == "main" {
                let mut ch = ctx.channel.lock().await;
                ch.post(i18n::SYS_SENDER_SYSTEM, i18n::CMD_CANNOT_REMOVE_MAIN, true);
                return CommandResult::Ok;
            }
            {
                let mut map = ctx.clients.lock().await;
                if let Some(client) = map.remove(&name) {
                    let mut c = client.lock().await;
                    c.stop().await;
                }
            }
            let mut ch = ctx.channel.lock().await;
            ch.remove_agent(&name);
            CommandResult::Ok
        }
        "/list" | "/ls" => {
            let mut ch = ctx.channel.lock().await;
            let agents = ch.list_agents();
            let info: Vec<String> = agents
                .iter()
                .map(|a| format!("{} ({}) {}", a.name, a.kind, a.status))
                .collect();
            if info.is_empty() {
                ch.post(i18n::SYS_SENDER_SYSTEM, i18n::CMD_NO_AGENTS, true);
            } else {
                ch.post(i18n::SYS_SENDER_SYSTEM, &info.join("  |  "), true);
            }
            CommandResult::Ok
        }
        "/adapters" => {
            let adapters = acp_core::adapter::list_detailed();
            let info: Vec<String> = adapters
                .iter()
                .map(|(name, desc)| format!("{name}: {desc}"))
                .collect();
            let mut ch = ctx.channel.lock().await;
            ch.post(i18n::SYS_SENDER_SYSTEM, &info.join("\n"), true);
            CommandResult::Ok
        }
        "/cancel" => {
            if parts.len() < 2 {
                let mut ch = ctx.channel.lock().await;
                ch.post(i18n::SYS_SENDER_SYSTEM, i18n::CMD_CANCEL_USAGE, true);
                return CommandResult::Ok;
            }
            let name = parts[1].to_string();
            let client = {
                let map = ctx.clients.lock().await;
                map.get(&name).cloned()
            };
            if let Some(client) = client {
                let c = client.lock().await;
                c.cancel().await;
                drop(c);
                let mut ch = ctx.channel.lock().await;
                ch.post(i18n::SYS_SENDER_SYSTEM, &i18n::cmd_cancelled(&name), true);
            } else {
                let mut ch = ctx.channel.lock().await;
                ch.post(i18n::SYS_SENDER_SYSTEM, &i18n::cmd_not_found(&name), true);
            }
            CommandResult::Ok
        }
        "/save" => {
            let mut ch = ctx.channel.lock().await;
            match acp_core::store::save(&ch).await {
                Ok(path) => {
                    ch.mark_saved();
                    ch.post(i18n::SYS_SENDER_SYSTEM, &i18n::cmd_saved(&path.display().to_string()), true);
                }
                Err(e) => {
                    ch.post(i18n::SYS_SENDER_SYSTEM, &i18n::cmd_save_failed(&e.to_string()), true);
                }
            }
            CommandResult::Ok
        }
        "/help" => {
            let mut ch = ctx.channel.lock().await;
            ch.post(
                "系统",
                "/add <name> <adapter>  添加 agent\n\
                 /remove <name>         移除 agent\n\
                 /list                  列出 agents\n\
                 /adapters              列出可用 adapters\n\
                 /cancel <name>         取消当前任务\n\
                 /save                  保存频道快照\n\
                 /quit                  退出\n\
                 消息中用 @name 路由到指定 agent\n\
                 无 @mention 的消息默认发给 main\n\
                 Tab 补全命令和 @agent",
                true,
            );
            CommandResult::Ok
        }
        "/quit" | "/q" => CommandResult::Quit,
        "/group" => {
            let mut ch = ctx.channel.lock().await;
            if parts.len() < 2 {
                ch.post(
                    "系统",
                    "/group create <name> <member1> <member2> ...\n\
                     /group add <name> <member>\n\
                     /group list\n\
                     /group remove <name> <member>",
                    true,
                );
                return CommandResult::Ok;
            }
            match parts[1] {
                "create" => {
                    if parts.len() < 3 {
                        ch.post("系统", "用法: /group create <name> <members...>", true);
                        return CommandResult::Ok;
                    }
                    let gname = parts[2];
                    if ch.create_group(gname, "you") {
                        // Add additional members from parts[3..]
                        if parts.len() > 3 {
                            let members_str = parts[3];
                            for m in members_str.split_whitespace() {
                                if let Some(g) = ch.groups.get_mut(gname) {
                                    g.add_member(m);
                                }
                            }
                        }
                        let members = ch
                            .groups
                            .get(gname)
                            .map(|g| g.members.iter().cloned().collect::<Vec<_>>().join(", "))
                            .unwrap_or_default();
                        ch.post("系统", &format!("群组 [{gname}] 成员: {members}"), true);
                    } else {
                        ch.post("系统", &format!("群组 {gname} 已存在"), true);
                    }
                }
                "add" => {
                    if parts.len() < 4 {
                        ch.post("系统", "用法: /group add <name> <member>", true);
                        return CommandResult::Ok;
                    }
                    let gname = parts[2];
                    let member = parts[3];
                    if let Some(g) = ch.groups.get_mut(gname) {
                        g.add_member(member);
                        ch.post("系统", &format!("{member} 已加入群组 [{gname}]"), true);
                    } else {
                        ch.post("系统", &format!("群组 {gname} 不存在"), true);
                    }
                }
                "list" => {
                    if ch.groups.is_empty() {
                        ch.post("系统", "无群组", true);
                    } else {
                        let info: Vec<String> = ch
                            .groups
                            .iter()
                            .map(|(name, g)| {
                                let members: Vec<_> = g.members.iter().cloned().collect();
                                format!("[{name}] ({}人): {}", g.members.len(), members.join(", "))
                            })
                            .collect();
                        ch.post("系统", &info.join("\n"), true);
                    }
                }
                "remove" => {
                    if parts.len() < 4 {
                        ch.post("系统", "用法: /group remove <name> <member>", true);
                        return CommandResult::Ok;
                    }
                    let gname = parts[2];
                    let member = parts[3];
                    if let Some(g) = ch.groups.get_mut(gname) {
                        if g.remove_member(member) {
                            ch.post("系统", &format!("{member} 已退出群组 [{gname}]"), true);
                        } else {
                            ch.post(
                                "系统",
                                &format!("无法移除 {member}（创建者不能退出）"),
                                true,
                            );
                        }
                    } else {
                        ch.post("系统", &format!("群组 {gname} 不存在"), true);
                    }
                }
                _ => {
                    ch.post("系统", "未知子命令，用 /group 查看帮助", true);
                }
            }
            CommandResult::Ok
        }
        _ => {
            let mut ch = ctx.channel.lock().await;
            ch.post("系统", &format!("未知命令: {cmd}，/help 查看帮助"), true);
            CommandResult::Ok
        }
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::channel::Channel;

/// Global registry of active channels.
pub struct Registry {
    channels: HashMap<String, Arc<Mutex<Channel>>>,
    active_channel_id: Option<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            active_channel_id: None,
        }
    }

    pub fn add(&mut self, channel: Channel) -> String {
        let id = channel.channel_id.clone();
        self.channels
            .insert(id.clone(), Arc::new(Mutex::new(channel)));
        self.active_channel_id = Some(id.clone());
        id
    }

    pub fn get(&self, id: &str) -> Option<Arc<Mutex<Channel>>> {
        self.channels.get(id).cloned()
    }

    pub fn active(&self) -> Option<Arc<Mutex<Channel>>> {
        self.active_channel_id.as_ref().and_then(|id| self.get(id))
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active_channel_id.as_deref()
    }

    pub fn set_active(&mut self, id: &str) {
        if self.channels.contains_key(id) {
            self.active_channel_id = Some(id.to_string());
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Arc<Mutex<Channel>>> {
        let ch = self.channels.remove(id);
        if self.active_channel_id.as_deref() == Some(id) {
            self.active_channel_id = self.channels.keys().next().cloned();
        }
        ch
    }

    pub fn list(&self) -> Vec<ChannelSummary> {
        self.channels
            .iter()
            .map(|(id, _)| ChannelSummary {
                id: id.clone(),
                is_active: self.active_channel_id.as_deref() == Some(id.as_str()),
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ChannelSummary {
    pub id: String,
    pub is_active: bool,
}

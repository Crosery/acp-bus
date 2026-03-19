use serde::{Deserialize, Serialize};

/// ACP content block types (aligned with codecompanion)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ResourceLink {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Resource {
        resource: ResourceContent,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    Audio {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ContentBlock {
    /// Extract renderable text from a content block (aligned with client.lua get_renderable_text)
    pub fn renderable_text(&self) -> Option<String> {
        match self {
            ContentBlock::Text { text } => Some(text.clone()),
            ContentBlock::ResourceLink { uri, .. } => Some(format!("[resource: {uri}]")),
            ContentBlock::Resource { resource } => {
                if let Some(text) = &resource.text {
                    Some(text.clone())
                } else if let Some(uri) = &resource.uri {
                    Some(format!("[resource: {uri}]"))
                } else {
                    None
                }
            }
            ContentBlock::Image { .. } => Some("[image]".to_string()),
            ContentBlock::Audio { .. } => Some("[audio]".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_block_renderable() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        assert_eq!(block.renderable_text(), Some("hello".into()));
    }

    #[test]
    fn resource_link_renderable() {
        let block = ContentBlock::ResourceLink {
            uri: "file:///a.txt".into(),
            name: None,
        };
        assert_eq!(
            block.renderable_text(),
            Some("[resource: file:///a.txt]".into())
        );
    }

    #[test]
    fn serde_roundtrip() {
        let block = ContentBlock::Text {
            text: "test".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.renderable_text(), Some("test".into()));
    }
}

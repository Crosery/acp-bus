use acp_protocol::LineBuffer;

pub struct StdioTransport {
    pub line_buf: LineBuffer,
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            line_buf: LineBuffer::new(),
        }
    }
}

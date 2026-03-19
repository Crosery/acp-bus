use acp_protocol::LineBuffer;

pub struct StdioTransport {
    pub line_buf: LineBuffer,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            line_buf: LineBuffer::new(),
        }
    }
}

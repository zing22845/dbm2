//! Context picker sub-module messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerMsg {
    Message(ContextPickerMessage),
}

impl From<ContextPickerMessage> for ContextPickerMsg {
    fn from(m: ContextPickerMessage) -> Self {
        ContextPickerMsg::Message(m)
    }
}

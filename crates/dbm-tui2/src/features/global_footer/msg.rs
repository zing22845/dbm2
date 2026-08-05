//! Global footer feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterMsg {
    Message(FooterMessage),
}

impl From<FooterMessage> for FooterMsg {
    fn from(m: FooterMessage) -> Self {
        FooterMsg::Message(m)
    }
}

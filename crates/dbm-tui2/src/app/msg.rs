//! Central message router. `AppMsg` is the single enumeration through which
//! every feature message, shell message and no-op flows. Each feature owns
//! its `*Msg` enum and provides a `From<*Msg> for AppMsg` conversion so it
//! can be lifted into the router (Nested TEA + central message router).

use crate::app_shell::msg::ShellMsg;
use crate::features::discover::msg::DiscoverMsg;
use crate::features::explorer::msg::ExplorerMsg;
use crate::features::global_footer::msg::FooterMsg;
use crate::features::header::msg::HeaderMsg;
use crate::features::instance_workspace::msg::IwMsg;
use crate::features::perf_monitor::msg::PerfMsg;
use crate::features::sql_workspace::msg::SqlMsg;

use super::state::ModalKind;

/// The global message type. All updates are dispatched on this enum.
#[derive(Debug, Clone)]
pub enum AppMsg {
    /// Shell-owned messages (quit, tick, focus change).
    Shell(ShellMsg),
    /// Open a data-carrying modal (row-limit picker, page input, confirm,
    /// commit preview). The shell sets `state.modal` in `update`.
    OpenModal(ModalKind),
    /// Close the currently open modal.
    CloseModal,
    /// Header feature messages.
    Header(HeaderMsg),
    /// Explorer feature messages.
    Explorer(ExplorerMsg),
    /// Discover feature messages.
    Discover(DiscoverMsg),
    /// Instance workspace feature messages.
    Iw(IwMsg),
    /// SQL workspace feature messages.
    Sql(SqlMsg),
    /// Global footer feature messages.
    Footer(FooterMsg),
    /// Performance monitor feature messages.
    Perf(PerfMsg),
}

impl From<ShellMsg> for AppMsg {
    fn from(m: ShellMsg) -> Self {
        AppMsg::Shell(m)
    }
}
impl From<HeaderMsg> for AppMsg {
    fn from(m: HeaderMsg) -> Self {
        AppMsg::Header(m)
    }
}
impl From<ExplorerMsg> for AppMsg {
    fn from(m: ExplorerMsg) -> Self {
        AppMsg::Explorer(m)
    }
}
impl From<DiscoverMsg> for AppMsg {
    fn from(m: DiscoverMsg) -> Self {
        AppMsg::Discover(m)
    }
}
impl From<IwMsg> for AppMsg {
    fn from(m: IwMsg) -> Self {
        AppMsg::Iw(m)
    }
}
impl From<SqlMsg> for AppMsg {
    fn from(m: SqlMsg) -> Self {
        AppMsg::Sql(m)
    }
}
impl From<FooterMsg> for AppMsg {
    fn from(m: FooterMsg) -> Self {
        AppMsg::Footer(m)
    }
}
impl From<PerfMsg> for AppMsg {
    fn from(m: PerfMsg) -> Self {
        AppMsg::Perf(m)
    }
}

//! The global `Action` enum. An action is the result of running an `Effect`
//! and is fed back into the router. It is either a dispatched message or a
//! shell command.

use crate::app::msg::AppMsg;
use crate::app_shell::action::ShellAction;
use crate::features::discover::effect::DiscoverAction;
use crate::features::explorer::effect::ExplorerAction;
use crate::features::global_footer::effect::FooterAction;
use crate::features::header::effect::HeaderAction;
use crate::features::instance_workspace::effect::IwAction;
use crate::features::perf_monitor::effect::PerfAction;
use crate::features::sql_workspace::effect::SqlAction;

/// The global action type. Every effect action lifts into this enum.
#[derive(Debug, Clone)]
pub enum Action {
    /// Dispatch a message back into the central router.
    Dispatch(AppMsg),
    /// A shell command (quit, no-op, ...).
    Shell(ShellAction),
    /// Header feature action.
    Header(HeaderAction),
    /// Explorer feature action.
    Explorer(ExplorerAction),
    /// Discover feature action.
    Discover(DiscoverAction),
    /// Instance workspace feature action.
    Iw(IwAction),
    /// SQL workspace feature action.
    Sql(SqlAction),
    /// Global footer feature action.
    Footer(FooterAction),
    /// Performance monitor feature action.
    Perf(PerfAction),
}

impl From<AppMsg> for Action {
    fn from(m: AppMsg) -> Self {
        Action::Dispatch(m)
    }
}
impl From<ShellAction> for Action {
    fn from(a: ShellAction) -> Self {
        Action::Shell(a)
    }
}
impl From<HeaderAction> for Action {
    fn from(a: HeaderAction) -> Self {
        Action::Header(a)
    }
}
impl From<ExplorerAction> for Action {
    fn from(a: ExplorerAction) -> Self {
        Action::Explorer(a)
    }
}
impl From<DiscoverAction> for Action {
    fn from(a: DiscoverAction) -> Self {
        Action::Discover(a)
    }
}
impl From<IwAction> for Action {
    fn from(a: IwAction) -> Self {
        Action::Iw(a)
    }
}
impl From<crate::features::instance_workspace::connections::effect::ConnectionsAction> for Action {
    fn from(a: crate::features::instance_workspace::connections::effect::ConnectionsAction) -> Self {
        Action::Iw(IwAction::Connections(a))
    }
}
impl From<crate::features::explorer::instances::effect::InstancesAction> for Action {
    fn from(a: crate::features::explorer::instances::effect::InstancesAction) -> Self {
        Action::Explorer(crate::features::explorer::effect::ExplorerAction::Instances(a))
    }
}
impl From<SqlAction> for Action {
    fn from(a: SqlAction) -> Self {
        Action::Sql(a)
    }
}
impl From<FooterAction> for Action {
    fn from(a: FooterAction) -> Self {
        Action::Footer(a)
    }
}
impl From<PerfAction> for Action {
    fn from(a: PerfAction) -> Self {
        Action::Perf(a)
    }
}

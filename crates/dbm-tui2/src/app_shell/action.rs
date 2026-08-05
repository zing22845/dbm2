//! Shell-level actions. An `Action` is the result of running an `Effect`:
//! it is either a dispatched message (feed back into the router) or a
//! shell command (e.g. quit).

/// Actions owned by the shell.
#[derive(Debug, Clone)]
pub enum ShellAction {
    /// Ask the application to terminate.
    Quit,
}

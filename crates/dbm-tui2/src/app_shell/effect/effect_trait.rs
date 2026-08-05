//! The `Effect` trait. An effect performs an asynchronous side effect and
//! yields one or more actions. The associated `Action` type must convert
//! into the application's global action type (typically `crate::app::action::Action`)
//! via `From`.
//!
//! This module defines two concepts:
//! - `Effect`: the *business* trait that each feature implements. It describes
//!   "given this side effect, what list of actions should be produced?".
//!   Business developers only ever need to implement this trait.
//! - `ErasedEffect<A>`: the *infrastructure* type-erasure layer. It is only used
//!   by the central router (`UpdateResult`, `EffectRunner`) to store
//!   heterogeneous effects in a single `Vec` and execute them uniformly. The
//!   generic `A` is the application's global action type, resolved at the app
//!   layer so `app_shell` stays free of any dependency on `app`.

use std::future::Future;
use std::pin::Pin;

/// A boxed, `Send` future resolving to a list of actions. Using an explicit
/// boxed future (rather than `async fn`) keeps every effect future `Send`,
/// which the effect runner needs to spawn effects across threads.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// An effect performs side-effecting work and resolves to a list of actions.
pub trait Effect: Send + 'static {
    /// The action type produced by running this effect.
    type Action: Send + 'static;

    /// Run the effect, producing its resulting actions.
    fn run(self) -> BoxFuture<Vec<Self::Action>>;
}

/// A type-erased effect that resolves directly to the global action type `A`.
/// Implemented for any `Effect` whose action converts into `A`.
///
/// This is the erasure layer that lets the central router hold effects from
/// every feature in one heterogeneous `Vec<Box<dyn ErasedEffect<A>>>` and run
/// them without knowing their concrete types. Business code should not
/// implement it directly.
pub trait ErasedEffect<A>: Send + 'static {
    /// Run the effect and collect the global actions.
    fn run_erased(self: Box<Self>) -> BoxFuture<Vec<A>>;
}

impl<E, A> ErasedEffect<A> for E
where
    E: Effect + 'static,
    E::Action: Into<A>,
    A: Send + 'static,
{
    fn run_erased(self: Box<Self>) -> BoxFuture<Vec<A>> {
        Box::pin(async move {
            self.run()
                .await
                .into_iter()
                .map(Into::into)
                .collect()
        })
    }
}

//! The `Effect` trait. An effect performs an asynchronous side effect and
//! yields zero or more actions. The associated `Action` type must convert into
//! the application's global action type (typically `crate::app::action::Action`)
//! via `From`.
//!
//! This module defines three concepts:
//! - `Effect`: the *business* trait that each feature implements. It describes
//!   "given this side effect, what actions should be produced?". Business
//!   developers only ever need to implement this trait.
//! - `Emitter`: a streaming action channel handed to an effect. An effect can
//!   emit actions **while it runs** (e.g. progress updates for a long scan)
//!   through the emitter and still return any final actions from `run`. The
//!   emitter is `Send + Clone`, so it can be cloned into a background task.
//! - `ErasedEffect<A>`: the *infrastructure* type-erasure layer. It is only used
//!   by the central router (`UpdateResult`, `EffectRunner`) to store
//!   heterogeneous effects in a single `Vec` and execute them uniformly. The
//!   generic `A` is the application's global action type, resolved at the app
//!   layer so `app_shell` stays free of any dependency on `app`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::common::service::services::Services;

/// A boxed, `Send` future resolving to a list of actions. Using an explicit
/// boxed future (rather than `async fn`) keeps every effect future `Send`,
/// which the effect runner needs to spawn effects across threads.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Streaming action emitter handed to an effect's `run`.
///
/// Clone it into a background task and call [`Emitter::emit`] to push actions
/// (e.g. progress) back into the runner *before* `run` returns. Sending is
/// fire-and-forget: if the receiver is gone (shutting down), the emit is
/// dropped.
#[derive(Clone)]
pub struct Emitter<B> {
    inner: Arc<dyn Fn(B) + Send + Sync>,
}

impl<B> Emitter<B>
where
    B: Send + Sync + 'static,
{
    /// Build an emitter that sends `B` values into a channel sender.
    pub fn from_sender<A>(tx: mpsc::UnboundedSender<A>) -> Emitter<B>
    where
        B: Into<A>,
        A: Send + 'static,
    {
        Emitter {
            inner: Arc::new(move |b: B| {
                if tx.send(b.into()).is_err() {
                    // The runner's receiver is gone (app shutting down). The
                    // emit is dropped; log at debug so a dropped action is
                    // diagnosable without spamming in normal operation.
                    tracing::debug!("effect emit dropped: action receiver closed");
                }
            }),
        }
    }

    /// Adapt to an emitter over `C`, converting each emitted `C` via `Into<B>`.
    pub fn map<C>(&self) -> Emitter<C>
    where
        C: Into<B>,
        C: Send + Sync + 'static,
    {
        let base = self.inner.clone();
        Emitter {
            inner: Arc::new(move |c: C| base(c.into())),
        }
    }

    /// Push one action. Never blocks; a dropped receiver is silently ignored.
    pub fn emit(&self, action: B) {
        (self.inner)(action);
    }
}

/// An effect performs side-effecting work, emitting actions as it runs and
/// resolving to a final list of actions.
pub trait Effect: Send + 'static {
    /// The action type produced by running this effect.
    type Action: Send + Sync + 'static;

    /// Run the effect. `services` provides access to infrastructure (store,
    /// file IO, ...), `emit` forwards streaming actions (e.g. progress) while
    /// running, and the returned `Vec` holds any final actions.
    fn run(self, emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>>;
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
    fn run_erased(self: Box<Self>, emit: Emitter<A>, services: Arc<Services>) -> BoxFuture<Vec<A>>;
}

impl<E, A> ErasedEffect<A> for E
where
    E: Effect + 'static,
    E::Action: Into<A>,
    A: Send + Sync + 'static,
{
    fn run_erased(self: Box<Self>, emit: Emitter<A>, services: Arc<Services>) -> BoxFuture<Vec<A>> {
        Box::pin(async move {
            // Give the feature effect an emitter over its own action type; each
            // emitted action is converted to the global `A` on send.
            let emit = emit.map::<E::Action>();
            self.run(emit, services)
                .await
                .into_iter()
                .map(Into::into)
                .collect()
        })
    }
}

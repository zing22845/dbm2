//! The effect runner. It owns a channel of boxed effects and executes them
//! concurrently, forwarding the resulting global actions back into the
//! router. The generic `A` is the application's global action type, so this
//! infrastructure has no dependency on `app`.

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::effect_trait::ErasedEffect;

/// Handle used to submit effects. Internally wraps an mpsc sender of boxed
/// effects that resolve to the global action type `A`.
#[derive(Debug, Clone)]
pub struct EffectRunner<A> {
    tx: mpsc::UnboundedSender<Box<dyn ErasedEffect<A>>>,
}

impl<A> EffectRunner<A>
where
    A: Send + 'static,
{
    /// Create a new runner. Effects are executed on a background task set and
    /// their resulting actions are forwarded to `action_tx`.
    pub fn new(action_tx: mpsc::UnboundedSender<A>) -> (Self, EffectHandle<A>) {
        let (eff_tx, eff_rx) = mpsc::unbounded_channel::<Box<dyn ErasedEffect<A>>>();
        let runner = EffectRunner { tx: eff_tx };
        let handle = EffectHandle { eff_rx, action_tx };
        (runner, handle)
    }

    /// Submit an already-erased effect for execution.
    pub fn submit(&self, effect: Box<dyn ErasedEffect<A>>) {
        if self.tx.send(effect).is_err() {
            tracing::warn!("effect submitted after the runner was dropped; dropping it");
        }
    }
}

/// Background handle that drains the effect channel and forwards actions.
pub struct EffectHandle<A> {
    eff_rx: mpsc::UnboundedReceiver<Box<dyn ErasedEffect<A>>>,
    action_tx: mpsc::UnboundedSender<A>,
}

impl<A> EffectHandle<A>
where
    A: Send + 'static,
{
    /// Run the effect-draining loop. Spawns one task per effect and forwards
    /// every resulting action into `action_tx`.
    pub async fn run(self) {
        let mut set = JoinSet::new();
        let mut rx = self.eff_rx;
        while let Some(boxed) = rx.recv().await {
            let action_tx = self.action_tx.clone();
            set.spawn(async move {
                let actions = boxed.run_erased().await;
                for a in actions {
                    if action_tx.send(a).is_err() {
                        // The action receiver is gone (the app is shutting
                        // down or the loop dropped it); nothing to do.
                        tracing::warn!("action send failed: receiver closed");
                        break;
                    }
                }
            });
        }
        while let Some(res) = set.join_next().await {
            let _ = res;
        }
    }
}

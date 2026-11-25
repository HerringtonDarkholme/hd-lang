//! One process broker for portable signals and cancellation. Registration
//! and removal use one kind's ordered index; delivery visits only live clients.
//! `BTreeMap` keeps delivery O(live clients), even after many cancelled waits;
//! a sparse hash table would also visit its retained empty buckets.
//! signal-hook's conditional default restores the OS action while unclaimed.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Interrupt,
    Terminate,
    Hangup,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::Interrupt, Self::Terminate, Self::Hangup];
    #[cfg(unix)]
    #[must_use]
    pub fn os_number(self) -> i32 {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        match self {
            Self::Interrupt => SIGINT,
            Self::Terminate => SIGTERM,
            Self::Hangup => SIGHUP,
        }
    }
}

type Handler = Arc<dyn Fn(Kind) + Send + Sync>;
enum Callback {
    Wait(Box<dyn FnOnce() + Send>),
    Fallback(Handler),
    Forward(Handler),
}
#[derive(Clone, Copy)]
enum Role {
    Wait,
    Fallback,
    Forward,
}
#[derive(Default)]
struct Listeners {
    waits: BTreeMap<usize, Box<dyn FnOnce() + Send>>,
    fallbacks: BTreeMap<usize, Handler>,
    forwards: BTreeMap<usize, Handler>,
}
struct Broker {
    kinds: Mutex<[Listeners; 3]>,
    defaults: [Arc<AtomicBool>; 3],
    #[cfg(test)]
    probes: std::sync::atomic::AtomicUsize,
}

/// Dropping a subscription removes its ownership before the next signal.
/// The token's address stays unique until removal, including after delivery;
/// registrations therefore need no monotonically growing handle space.
pub struct Subscription {
    broker: Arc<Broker>,
    kind: Kind,
    role: Role,
    token: Arc<()>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let mut kinds = self
            .broker
            .kinds
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        #[cfg(test)]
        self.broker.probes.fetch_add(1, Ordering::SeqCst);
        let listeners = &mut kinds[self.kind as usize];
        let key = Arc::as_ptr(&self.token).addr();
        match self.role {
            Role::Wait => {
                listeners.waits.remove(&key);
            }
            Role::Fallback => {
                listeners.fallbacks.remove(&key);
            }
            Role::Forward => {
                listeners.forwards.remove(&key);
            }
        }
        self.broker.update_default(self.kind, listeners);
    }
}
impl Broker {
    fn new() -> Self {
        Self {
            kinds: Mutex::new(std::array::from_fn(|_| Listeners::default())),
            defaults: std::array::from_fn(|_| Arc::new(AtomicBool::new(true))),
            #[cfg(test)]
            probes: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn update_default(&self, kind: Kind, listeners: &Listeners) {
        self.defaults[kind as usize].store(
            listeners.waits.is_empty()
                && listeners.fallbacks.is_empty()
                && listeners.forwards.is_empty(),
            Ordering::SeqCst,
        );
    }
    fn subscribe(self: &Arc<Self>, kind: Kind, callback: Callback) -> Subscription {
        let token = Arc::new(());
        let role = match &callback {
            Callback::Wait(_) => Role::Wait,
            Callback::Fallback(_) => Role::Fallback,
            Callback::Forward(_) => Role::Forward,
        };
        #[cfg(test)]
        self.probes.fetch_add(1, Ordering::SeqCst);
        let mut kinds = self.kinds.lock().unwrap_or_else(PoisonError::into_inner);
        // Publish ownership before the entry: a concurrent OS notification
        // waits on this mutex before dispatching to the new subscriber.
        self.defaults[kind as usize].store(false, Ordering::SeqCst);
        let key = Arc::as_ptr(&token).addr();
        match callback {
            Callback::Wait(wait) => {
                kinds[kind as usize].waits.insert(key, wait);
            }
            Callback::Fallback(handler) => {
                kinds[kind as usize].fallbacks.insert(key, handler);
            }
            Callback::Forward(handler) => {
                kinds[kind as usize].forwards.insert(key, handler);
            }
        }
        Subscription {
            broker: Arc::clone(self),
            kind,
            role,
            token,
        }
    }
    fn deliver(&self, kind: Kind) {
        #[cfg(test)]
        self.probes.fetch_add(1, Ordering::SeqCst);
        let (waits, fallbacks, forwards) = {
            let mut kinds = self.kinds.lock().unwrap_or_else(PoisonError::into_inner);
            let listeners = &mut kinds[kind as usize];
            let waits = std::mem::take(&mut listeners.waits);
            let fallbacks: Vec<_> = if waits.is_empty() {
                listeners.fallbacks.values().cloned().collect()
            } else {
                Vec::new()
            };
            let forwards: Vec<_> = listeners.forwards.values().cloned().collect();
            self.update_default(kind, listeners);
            (waits, fallbacks, forwards)
        };
        if waits.is_empty() && fallbacks.is_empty() && forwards.is_empty() {
            default_action(kind);
        }
        // User callbacks never run under the registry lock. A signal with
        // current waiters is handled once, without cancelling other entries.
        // A child process is a separate signal owner. Its transport still
        // receives the event when a local program has handled this kind.
        for callback in forwards {
            #[cfg(test)]
            self.probes.fetch_add(1, Ordering::SeqCst);
            callback(kind);
        }
        for callback in waits.into_values() {
            #[cfg(test)]
            self.probes.fetch_add(1, Ordering::SeqCst);
            callback();
        }
        for callback in fallbacks {
            #[cfg(test)]
            self.probes.fetch_add(1, Ordering::SeqCst);
            callback(kind);
        }
    }
}

fn shared() -> Result<Arc<Broker>, String> {
    static BROKER: OnceLock<Result<Arc<Broker>, String>> = OnceLock::new();
    BROKER
        .get_or_init(|| {
            let broker = Arc::new(Broker::new());
            #[cfg(unix)]
            {
                for kind in Kind::ALL {
                    signal_hook::flag::register_conditional_default(
                        kind.os_number(),
                        Arc::clone(&broker.defaults[kind as usize]),
                    )
                    .map_err(|e| e.to_string())?;
                }
                let mut signals =
                    signal_hook::iterator::Signals::new(Kind::ALL.map(Kind::os_number))
                        .map_err(|e| e.to_string())?;
                let source = Arc::clone(&broker);
                // cost: one listener thread per process, never one per wait.
                std::thread::Builder::new()
                    .name("hd-signals".into())
                    .spawn(move || {
                        for signal in signals.forever() {
                            if let Some(kind) = Kind::ALL
                                .into_iter()
                                .find(|kind| kind.os_number() == signal)
                            {
                                source.deliver(kind);
                            }
                        }
                    })
                    .map_err(|e| e.to_string())?;
            }
            #[cfg(windows)]
            {
                let source = Arc::clone(&broker);
                ctrlc::set_handler(move || source.deliver(Kind::Interrupt))
                    .map_err(|e| e.to_string())?;
            }
            Ok(broker)
        })
        .clone()
}

#[cfg(unix)]
fn default_action(kind: Kind) {
    if let Err(error) = signal_hook::low_level::emulate_default_handler(kind.os_number()) {
        eprintln!("signal default action failed: {error}");
        std::process::exit(1);
    }
}
#[cfg(windows)]
fn default_action(_kind: Kind) {
    // STATUS_CONTROL_C_EXIT: no hd cleanup runs on the console default.
    std::process::exit(i32::from_ne_bytes(0xC000_013Au32.to_ne_bytes()));
}

/// Complete all current waits of `kind` on its next notification.
pub fn wait(kind: Kind, callback: impl FnOnce() + Send + 'static) -> Result<Subscription, String> {
    Ok(shared()?.subscribe(kind, Callback::Wait(Box::new(callback))))
}
/// Used only when no Signals waiter owns the delivered kind. Repeated
/// notifications reach the same scoped cancellation/forwarding handler.
pub fn fallback(
    kinds: &[Kind],
    callback: impl Fn(Kind) + Send + Sync + 'static,
) -> Result<Vec<Subscription>, String> {
    persistent(kinds, &(Arc::new(callback) as Handler), false)
}
/// Forward notifications to another process without making its handling
/// decision here. Local waits still take priority over local cancellation.
pub fn forward(
    kinds: &[Kind],
    callback: impl Fn(Kind) + Send + Sync + 'static,
) -> Result<Vec<Subscription>, String> {
    persistent(kinds, &(Arc::new(callback) as Handler), true)
}
fn persistent(
    kinds: &[Kind],
    callback: &Handler,
    forward: bool,
) -> Result<Vec<Subscription>, String> {
    let broker = shared()?;
    let mut selected = [false; 3]; // The three portable Signal variants.
    let mut subscriptions = Vec::new();
    for &kind in kinds {
        if !selected[kind as usize] {
            selected[kind as usize] = true;
            let callback = if forward {
                Callback::Forward(Arc::clone(callback))
            } else {
                Callback::Fallback(Arc::clone(callback))
            };
            subscriptions.push(broker.subscribe(kind, callback));
        }
    }
    Ok(subscriptions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn one_delivery_wakes_matching_waits_without_cancelling_the_entry() {
        let broker = Arc::new(Broker::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&cancelled);
        let cancel = broker.subscribe(
            Kind::Terminate,
            Callback::Fallback(Arc::new(move |kind| {
                assert_eq!(kind, Kind::Terminate);
                count.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let mut leases = Vec::new();
        for _ in 0..1000 {
            let count = Arc::clone(&calls);
            leases.push(broker.subscribe(
                Kind::Terminate,
                Callback::Wait(Box::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                })),
            ));
        }
        broker.deliver(Kind::Terminate);
        assert_eq!(calls.load(Ordering::SeqCst), 1000);
        assert_eq!(cancelled.load(Ordering::SeqCst), 0);
        broker.deliver(Kind::Terminate);
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);
        drop(leases);
        drop(cancel);
        assert!(broker.defaults[Kind::Terminate as usize].load(Ordering::SeqCst));
    }

    #[test]
    fn forwarding_preserves_the_kind_without_cancelling_a_local_wait() {
        let broker = Arc::new(Broker::new());
        let forwarded = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&forwarded);
        let relay = broker.subscribe(
            Kind::Hangup,
            Callback::Forward(Arc::new(move |kind| {
                assert_eq!(kind, Kind::Hangup);
                count.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let wait = broker.subscribe(Kind::Hangup, Callback::Wait(Box::new(|| {})));
        let cancel = broker.subscribe(
            Kind::Hangup,
            Callback::Fallback(Arc::new(|_| panic!("a wait owns this kind"))),
        );
        broker.deliver(Kind::Hangup);
        assert_eq!(forwarded.load(Ordering::SeqCst), 1);
        drop(wait);
        drop(cancel);
        assert!(!broker.defaults[Kind::Hangup as usize].load(Ordering::SeqCst));
        drop(relay);
        assert!(broker.defaults[Kind::Hangup as usize].load(Ordering::SeqCst));
    }

    #[test]
    fn unregister_restores_default_and_a_cancelled_wait_never_completes() {
        let broker = Arc::new(Broker::new());
        let completed = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&completed);
        let cancel = broker.subscribe(Kind::Interrupt, Callback::Fallback(Arc::new(|_| {})));
        let waiting = broker.subscribe(
            Kind::Interrupt,
            Callback::Wait(Box::new(move || {
                done.store(true, Ordering::SeqCst);
            })),
        );
        drop(waiting);
        broker.deliver(Kind::Interrupt);
        assert!(!completed.load(Ordering::SeqCst));
        assert!(!broker.defaults[Kind::Interrupt as usize].load(Ordering::SeqCst));
        drop(cancel);
        assert!(broker.defaults[Kind::Interrupt as usize].load(Ordering::SeqCst));
    }

    #[test]
    fn registration_and_one_kind_delivery_cost_stays_flat_as_unrelated_waits_grow() {
        for width in [1000, 10_000] {
            let broker = Arc::new(Broker::new());
            let mut unrelated = Vec::new();
            for _ in 0..width {
                unrelated.push(broker.subscribe(
                    Kind::Hangup,
                    Callback::Wait(Box::new(|| panic!("unrelated signal delivered"))),
                ));
            }
            let before = broker.probes.load(Ordering::SeqCst);
            let matching = broker.subscribe(Kind::Interrupt, Callback::Wait(Box::new(|| {})));
            assert_eq!(broker.probes.load(Ordering::SeqCst) - before, 1);
            broker.deliver(Kind::Interrupt);
            assert_eq!(broker.probes.load(Ordering::SeqCst) - before, 3);
            assert_eq!(
                broker
                    .kinds
                    .lock()
                    .unwrap()
                    .get(Kind::Hangup as usize)
                    .unwrap()
                    .waits
                    .len(),
                width
            );
            drop(matching);
            assert_eq!(broker.probes.load(Ordering::SeqCst) - before, 4);
            assert!(broker.defaults[Kind::Interrupt as usize].load(Ordering::SeqCst));
            assert!(!broker.defaults[Kind::Hangup as usize].load(Ordering::SeqCst));
        }
    }
    #[cfg(unix)]
    #[test]
    fn last_native_waiter_restores_the_os_default() {
        use std::os::unix::process::ExitStatusExt as _;
        use std::time::{Duration, Instant};
        const CHILD: &str = "HD_SIGNAL_DEFAULT_CHILD";
        if let Ok(mode) = std::env::var(CHILD) {
            let (sent, received) = std::sync::mpsc::channel();
            let subscription = wait(Kind::Interrupt, move || {
                let _ = sent.send(());
            })
            .unwrap();
            if mode == "delivered" {
                signal_hook::low_level::raise(Kind::Interrupt.os_number()).unwrap();
                received.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            drop(subscription);
            // Both delivery and cancellation leave no owner. This notification
            // must terminate this isolated child, not return or hang.
            signal_hook::low_level::raise(Kind::Interrupt.os_number()).unwrap();
            panic!("the default signal action did not terminate the child");
        }
        for mode in ["delivered", "cancelled"] {
            // cost: one isolated child for each of two default-action paths.
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "signals::tests::last_native_waiter_restores_the_os_default",
                    "--nocapture",
                ])
                .env(CHILD, mode)
                .stdout(std::process::Stdio::null())
                // cost: the one child of this delivery/cancellation path.
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("native default-action child timed out: {mode}");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.signal(), Some(Kind::Interrupt.os_number()), "{mode}");
        }
    }
}

use crate::{
    config::Config,
    docker::{Engine, Health},
    store::{Instance, Lease, Phase, State, Store, secret},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug)]
pub struct Fault {
    pub status: u16,
    pub message: String,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Fault {}
fn fault(status: u16, message: &str) -> anyhow::Error {
    Fault {
        status,
        message: message.into(),
    }
    .into()
}

pub struct Controller {
    pub timeline: crate::timeline::Timeline,
    pub deployment: crate::deployment::Tracker,
    pub maintenance: Arc<crate::maintenance::Gate>,
    pub probation: Arc<crate::probation::Gate>,
    pub last_tick: std::sync::atomic::AtomicU64,
    pub config: Config,
    pub engine: Arc<dyn Engine>,
    inner: Mutex<(Store, State)>,
    reconcile_lock: tokio::sync::Mutex<()>,
    operations: Mutex<BTreeMap<String, tokio::task::JoinHandle<Outcome>>>,
    cursor: Mutex<String>,
    upstreams: Mutex<BTreeMap<String, (String, std::net::SocketAddr)>>,
    operator_games: BTreeSet<String>,
}

impl Controller {
    pub fn open(mut config: Config, engine: Arc<dyn Engine>) -> Result<Self> {
        config.validate()?;
        let operator_games = config.games.keys().cloned().collect();
        let (store, mut state) = Store::open(&config.controller.state_dir)?;
        for (id, game) in &state.published_games {
            ensure!(
                !config.games.contains_key(id),
                "published app conflicts with operator catalog: {id}"
            );
            config.games.insert(id.clone(), game.clone());
        }
        config.validate()?;
        for (id, instance) in &state.games {
            ensure!(
                !instance.reserved() || config.games.get(id) == Some(&instance.spec),
                "stop {id} before changing or removing its manifest"
            );
            ensure!(
                !instance.reserved()
                    || (config.controller.port_start..=config.controller.port_end)
                        .contains(&instance.port),
                "active game port is outside the configured range"
            );
        }
        state.games.retain(|id, _| config.games.contains_key(id));
        let mut ports: BTreeSet<u16> = state.games.values().map(|i| i.port).collect();
        ensure!(ports.len() == state.games.len(), "duplicate ports in state");
        for (id, game) in &config.games {
            if let Some(i) = state.games.get_mut(id) {
                ensure!(
                    (config.controller.port_start..=config.controller.port_end).contains(&i.port),
                    "remove sleeping games before changing the port range"
                );
                i.spec = game.clone();
            } else {
                let port = (config.controller.port_start..=config.controller.port_end)
                    .find(|p| !ports.contains(p))
                    .context("no free game ports")?;
                ports.insert(port);
                state.games.insert(
                    id.clone(),
                    Instance {
                        spec: game.clone(),
                        port,
                        phase: Phase::Sleeping,
                        attempt: String::new(),
                        name: String::new(),
                        started: 0,
                        idle_since: None,
                        retry_at: 0,
                        error: None,
                        leases: BTreeMap::new(),
                    },
                );
            }
        }
        let (memory, cpu) = Self::usage(&state);
        ensure!(
            memory <= config.controller.memory_budget_mb
                && cpu <= config.controller.cpu_budget_millis,
            "active reservations exceed the new budget"
        );
        store.save(&state)?;
        let probation = crate::probation::Gate::load(&config.controller.state_dir);
        Ok(Self {
            timeline: crate::timeline::Timeline::new(&config.controller),
            deployment: crate::deployment::Tracker::open(&config.controller.state_dir)?,
            maintenance: Default::default(),
            probation,
            last_tick: std::sync::atomic::AtomicU64::new(now()),
            config,
            engine,
            inner: Mutex::new((store, state)),
            reconcile_lock: tokio::sync::Mutex::new(()),
            operations: Default::default(),
            cursor: Default::default(),
            upstreams: Default::default(),
            operator_games,
        })
    }

    pub(crate) fn runtime_attempt(&self, id: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .1
            .games
            .get(id)
            .filter(|i| i.reserved())
            .map(|i| i.attempt.clone())
    }
    pub fn snapshot(&self) -> State {
        self.inner.lock().unwrap().1.clone()
    }

    /// Register a published server without restarting the daemon. Active worlds
    /// cannot change version: publishing must wait until their session ends.
    pub fn register(&self, id: &str, game: crate::config::Game) -> Result<()> {
        let mut validation = self.config.clone();
        validation.games.clear();
        validation.games.insert(id.into(), game.clone());
        validation.validate()?;
        self.mutate(|state| {
            if self.operator_games.contains(id) {
                return Err(fault(409, "app belongs to the operator catalog"));
            }
            if let Some(i) = state.games.get_mut(id) {
                if i.spec != game && i.reserved() {
                    return Err(fault(
                        409,
                        "players are using this server; retry after it sleeps",
                    ));
                }
                i.spec = game.clone();
            } else {
                let port = (self.config.controller.port_start..=self.config.controller.port_end)
                    .find(|p| !state.games.values().any(|i| i.port == *p))
                    .ok_or_else(|| fault(503, "no game ports available"))?;
                state.games.insert(
                    id.into(),
                    Instance {
                        spec: game.clone(),
                        port,
                        phase: Phase::Sleeping,
                        attempt: String::new(),
                        name: String::new(),
                        started: 0,
                        idle_since: None,
                        retry_at: 0,
                        error: None,
                        leases: BTreeMap::new(),
                    },
                );
            }
            state.published_games.insert(id.into(), game);
            Ok(())
        })
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let mut guard = self.inner.lock().unwrap();
        let mut next = guard.1.clone();
        let result = f(&mut next)?;
        if next != guard.1 {
            guard.0.save(&next)?;
            self.upstreams.lock().unwrap().retain(|id, (attempt, _)| {
                next.games
                    .get(id)
                    .is_some_and(|i| i.phase == Phase::Ready && &i.attempt == attempt)
            });
            guard.1 = next;
        }
        Ok(result)
    }

    fn usage(state: &State) -> (u64, u64) {
        state.games.values().filter(|i| i.reserved()).fold(
            state
                .build_reservation
                .as_ref()
                .map(|r| (r.memory_mb, r.cpu_millis))
                .unwrap_or((0, 0)),
            |(m, c), i| (m + i.spec.memory_mb, c + i.spec.cpu_millis),
        )
    }

    /// Packaging shares the same admission lock as games. Persist the reservation
    /// until its containers are removed, including across controller restarts.
    pub fn reserve_build(&self, job: &str, memory_mb: u64, cpu_millis: u64) -> Result<bool> {
        self.mutate(|state| {
            if state.build_reservation.is_some() {
                return Ok(false);
            }
            let (memory, cpu) = Self::usage(state);
            if memory.saturating_add(memory_mb) > self.config.controller.memory_budget_mb
                || cpu.saturating_add(cpu_millis) > self.config.controller.cpu_budget_millis
            {
                return Ok(false);
            }
            state.build_reservation = Some(crate::store::BuildReservation {
                job: job.into(),
                memory_mb,
                cpu_millis,
            });
            Ok(true)
        })
    }
    pub fn release_build(&self, job: &str) -> Result<()> {
        self.mutate(|state| {
            ensure!(
                state
                    .build_reservation
                    .as_ref()
                    .is_none_or(|r| r.job == job),
                "packaging reservation belongs to another job"
            );
            state.build_reservation = None;
            Ok(())
        })
    }

    pub async fn recover(&self) -> Result<()> {
        self.engine.preflight().await?;
        let state = self.snapshot();
        for (name, owner) in self.engine.inventory().await? {
            ensure!(
                owner == state.owner
                    && state.games.values().any(|i| i.reserved() && i.name == name),
                "untracked Infuser container {name}; restore its state before starting the controller"
            );
        }
        self.reconcile(now()).await?;
        Ok(())
    }

    fn view(&self, id: &str, i: &Instance, time: u64) -> Value {
        let endpoint = if i.phase == Phase::Ready {
            if i.spec.web {
                json!({"url": format!("https://{id}-server.{}", self.config.controller.domain)})
            } else {
                json!({"host": self.config.controller.public_host, "port": i.port, "protocols": i.spec.protocols})
            }
        } else {
            Value::Null
        };
        json!({"app_id": id, "state": i.phase, "endpoint": endpoint,
            "active_leases": i.leases.values().filter(|l| l.expires > time).count(),
            "error": i.error, "retry_at": i.retry_at})
    }

    pub fn list(&self, time: u64) -> Value {
        let state = self.snapshot();
        let (memory, cpu) = Self::usage(&state);
        json!({"apps": state.games.iter().map(|(id, i)| self.view(id, i, time)).collect::<Vec<_>>(),
            "reserved_memory_mb": memory, "reserved_cpu_millis": cpu})
    }

    pub fn acquire(&self, id: &str, session: &str, time: u64) -> Result<Value> {
        ensure!(
            !session.is_empty()
                && session.len() <= 128
                && session
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            Fault {
                status: 400,
                message: "session_id must be 1-128 letters, digits, underscores or hyphens".into()
            }
        );
        self.mutate(|state| {
            let (memory, cpu) = Self::usage(state);
            let i = state
                .games
                .get_mut(id)
                .ok_or_else(|| fault(404, "unknown app"))?;
            if i.phase == Phase::Stopping {
                return Err(fault(409, "app is stopping; retry shortly"));
            }
            if i.phase == Phase::Failed && time < i.retry_at {
                return Err(fault(503, "app is in failure backoff"));
            }
            if !i.reserved() {
                if memory + i.spec.memory_mb > self.config.controller.memory_budget_mb
                    || cpu + i.spec.cpu_millis > self.config.controller.cpu_budget_millis
                {
                    return Err(fault(
                        503,
                        "game capacity is full; retry after another game becomes idle",
                    ));
                }
                i.attempt = secret()[..16].into();
                i.name = format!("infuser-{}-{id}-{}", state.owner, i.attempt);
                i.phase = Phase::Starting;
                i.started = time;
                i.error = None;
                i.leases.clear();
            }
            i.leases.retain(|_, l| l.expires > time);
            let existing = i
                .leases
                .iter()
                .find(|(_, l)| l.session == session)
                .map(|(key, _)| key.clone());
            let key = if let Some(key) = existing {
                key
            } else {
                if i.leases.len() >= i.spec.max_leases {
                    return Err(fault(429, "app session limit reached"));
                }
                let key = secret()[..32].to_string();
                i.leases.insert(
                    key.clone(),
                    Lease {
                        session: session.into(),
                        token: secret(),
                        expires: 0,
                    },
                );
                key
            };
            i.idle_since = None;
            let lease = i.leases.get_mut(&key).unwrap();
            lease.expires = time + self.config.controller.lease_seconds;
            let token = lease.token.clone();
            let expires = lease.expires;
            let mut view = self.view(id, i, time);
            view["lease_id"] = json!(key);
            view["lease_token"] = json!(token);
            view["expires_at"] = json!(expires);
            view["heartbeat_after_seconds"] = json!(self.config.controller.lease_seconds / 3);
            Ok(view)
        })
    }

    pub fn lease(
        &self,
        id: &str,
        lease_id: &str,
        token: &str,
        action: &str,
        time: u64,
    ) -> Result<Value> {
        self.mutate(|state| {
            let i = state
                .games
                .get_mut(id)
                .ok_or_else(|| fault(404, "unknown app"))?;
            let l = i
                .leases
                .get_mut(lease_id)
                .ok_or_else(|| fault(404, "unknown lease"))?;
            if !bool::from(l.token.as_bytes().ct_eq(token.as_bytes())) {
                return Err(fault(401, "invalid lease token"));
            }
            if l.expires <= time {
                return Err(fault(410, "lease expired; launch again"));
            }
            if action == "delete" {
                i.leases.remove(lease_id);
                if i.leases.values().all(|l| l.expires <= time) {
                    i.idle_since = Some(time);
                }
            } else if action == "renew" {
                if !matches!(i.phase, Phase::Starting | Phase::Ready) {
                    return Err(fault(409, "app is no longer running"));
                }
                l.expires = time + self.config.controller.lease_seconds;
                i.idle_since = None;
            }
            let mut view = self.view(id, i, time);
            view["expires_at"] = json!(i.leases.get(lease_id).map(|l| l.expires));
            Ok(view)
        })
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        self.mutate(|state| {
            let i = state
                .games
                .get_mut(id)
                .ok_or_else(|| fault(404, "unknown app"))?;
            if i.reserved() {
                i.phase = Phase::Stopping;
                i.error = None;
            }
            i.leases.clear();
            Ok(())
        })
    }

    /// Read only verified routing for the current attempt. Never inspect Docker on
    /// the request path, and never reuse a persisted address after restart.
    pub fn web_address(&self, id: &str, attempt: &str) -> Result<std::net::SocketAddr> {
        let guard = self.inner.lock().unwrap();
        ensure!(
            guard
                .1
                .games
                .get(id)
                .is_some_and(|i| i.phase == Phase::Ready && i.attempt == attempt),
            fault(503, "game server is not ready")
        );
        self.upstreams
            .lock()
            .unwrap()
            .get(id)
            .filter(|(a, _)| a == attempt)
            .map(|(_, a)| *a)
            .ok_or_else(|| fault(503, "game server routing is warming up"))
    }
    /// Release only publisher registrations whose resources are confirmed gone.
    /// Worlds are deliberately retained and can be adopted by a later publication.
    pub fn unregister(&self, id: &str) -> Result<bool> {
        ensure!(
            !self.operator_games.contains(id),
            fault(409, "app belongs to the operator catalog")
        );
        if self.operations.lock().unwrap().contains_key(id) {
            return Ok(false);
        }
        self.mutate(|s| {
            if s.games.get(id).is_some_and(Instance::reserved) {
                return Ok(false);
            }
            s.published_games.remove(id);
            s.games.remove(id);
            Ok(true)
        })
    }
    fn prepare_operation(&self, id: &str, time: u64) -> Result<Option<Instance>> {
        self.mutate(|state| {
            let Some(i) = state.games.get_mut(id) else {
                return Ok(None);
            };
            let last_expiry = i.leases.values().map(|l| l.expires).max();
            i.leases.retain(|_, l| l.expires > time);
            if !i.reserved() {
                return Ok(None);
            }
            if i.leases.is_empty() && i.idle_since.is_none() {
                i.idle_since = Some(last_expiry.unwrap_or(time));
            }
            if i.phase != Phase::Stopping {
                if i.idle_since
                    .is_some_and(|since| time >= since.saturating_add(i.spec.idle_seconds))
                {
                    i.phase = Phase::Stopping;
                } else if i.phase == Phase::Starting
                    && time >= i.started.saturating_add(i.spec.startup_seconds)
                {
                    i.phase = Phase::Stopping;
                    i.error = Some("startup readiness deadline exceeded".into());
                }
            }
            Ok(Some(i.clone()))
        })
    }
    fn apply_operation(&self, outcome: Outcome, time: u64) -> Result<()> {
        let Outcome {
            id,
            instance,
            result,
            address,
        } = outcome;
        self.mutate(|state| {
                let Some(i) = state.games.get_mut(&id) else { return Ok(()); };
                // A stop request may arrive while Docker is working; it wins.
                if i.attempt != instance.attempt || i.phase != instance.phase {
                    return Ok(());
                }
                let before = i.phase.clone();
                match result {
                    Ok(_) if instance.phase == Phase::Stopping => {
                        i.phase = if i.error.is_some() {
                            Phase::Failed
                        } else {
                            Phase::Sleeping
                        };
                        i.retry_at = if i.error.is_some() {
                            time + self.config.controller.retry_seconds
                        } else {
                            0
                        };
                        i.idle_since = None;
                    }
                    Ok(Health::Ready) => {
                        i.phase = Phase::Ready;
                    }
                    Ok(Health::Starting) if instance.phase == Phase::Starting => {}
                    Ok(_) => {
                        i.phase = Phase::Stopping;
                        i.error = Some("game process exited or failed its health check".into());
                    }
                    Err(error) => {
                        tracing::warn!(app_id = %id, %error, "Docker operation failed");
                        // Ambiguous mutations retain the reservation until cleanup succeeds.
                        if instance.phase == Phase::Starting {
                            i.phase = Phase::Stopping;
                            i.error = Some(error.to_string());
                        }
                    }
                }
                if i.phase != before {
                    let context = crate::timeline::ContextFields {
                        app: id.clone(),
                        attempt: i.attempt.clone(),
                        ..Default::default()
                    };
                    let change = serde_json::json!({"state": i.phase, "previous": before});
                    self.timeline.record(crate::timeline::Event::server("runtime.transition", context, change, true));
                    tracing::info!(app_id = %id, attempt = %i.attempt, image = %i.spec.image,
                        from = ?before, to = ?i.phase, error = ?i.error, "Game lifecycle transition");
                }
                Ok(())
            })?;
        if let Some(address) = address {
            let guard = self.inner.lock().unwrap();
            if guard
                .1
                .games
                .get(&id)
                .is_some_and(|i| i.phase == Phase::Ready && i.attempt == instance.attempt)
            {
                self.upstreams
                    .lock()
                    .unwrap()
                    .insert(id, (instance.attempt, address));
            }
        }
        Ok(())
    }
    /// A complete sweep for explicit operator/test callers. Production uses the
    /// nonblocking step below, so one Docker stop never delays the next sweep.
    pub async fn reconcile(&self, time: u64) -> Result<()> {
        use futures_util::StreamExt;
        let _single = self.reconcile_lock.lock().await;
        let state = self.snapshot();
        let mut work = Vec::new();
        for id in state.games.keys() {
            if let Some(instance) = self.prepare_operation(id, time)? {
                work.push(operate(
                    self.engine.clone(),
                    id.clone(),
                    instance,
                    state.owner.clone(),
                ));
            }
        }
        let mut results = futures_util::stream::iter(work).buffer_unordered(2);
        while let Some(result) = results.next().await {
            self.apply_operation(result, time)?;
        }
        Ok(())
    }
    pub async fn reconcile_step(&self, time: u64) -> Result<()> {
        let _single = self.reconcile_lock.lock().await;
        let finished: Vec<_> = self
            .operations
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
            let task = self.operations.lock().unwrap().remove(&id).unwrap();
            self.apply_operation(task.await.context("lifecycle worker failed")?, time)?;
        }
        let state = self.snapshot();
        let cursor = self.cursor.lock().unwrap().clone();
        let mut ids: Vec<_> = state.games.keys().cloned().collect();
        ids.sort_by_key(|id| (id <= &cursor, id.clone()));
        for id in ids {
            // Expiry and stop transitions continue even while both workers are busy.
            let instance = self.prepare_operation(&id, time)?;
            let mut tasks = self.operations.lock().unwrap();
            if tasks.len() >= 2 || tasks.contains_key(&id) {
                continue;
            }
            if let Some(instance) = instance {
                *self.cursor.lock().unwrap() = id.clone();
                tasks.insert(
                    id.clone(),
                    tokio::spawn(operate(
                        self.engine.clone(),
                        id,
                        instance,
                        state.owner.clone(),
                    )),
                );
            }
        }
        self.last_tick
            .store(now(), std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}
struct Outcome {
    id: String,
    instance: Instance,
    result: Result<Health>,
    address: Option<std::net::SocketAddr>,
}
async fn operate(
    engine: Arc<dyn Engine>,
    id: String,
    instance: Instance,
    owner: String,
) -> Outcome {
    let mut result = match instance.phase {
        Phase::Starting => engine.ensure(&id, &instance, &owner).await,
        Phase::Ready => engine.health(&instance, &owner, &id).await,
        Phase::Stopping => engine
            .remove(&id, &instance, &owner)
            .await
            .map(|_| Health::Missing),
        _ => unreachable!(),
    };
    let mut address = None;
    if matches!(result, Ok(Health::Ready)) && instance.spec.web {
        match engine.web_address(&id, &instance, &owner).await {
            Ok(value) => address = Some(value),
            Err(error) => result = Err(error),
        }
    }
    Outcome {
        id,
        instance,
        result,
        address,
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        for task in self.operations.get_mut().unwrap().values() {
            task.abort();
        }
    }
}

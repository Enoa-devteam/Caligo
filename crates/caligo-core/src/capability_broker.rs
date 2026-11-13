//! Bounded, ephemeral read-only query correlation. Nothing here is journaled or retried.
//! Lock order: actor (caller) -> broker -> dispatch queue. The dispatch pump must
//! release the queue before consulting the broker. Waiters hold none of these locks.

use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Mutex,
};
use std::time::{Duration, Instant};

use caligo_model::capability::{
    HostQuery, QueryCapability, QueryOutcome, QueryScope, MAX_QUERY_TIMEOUT_MS,
};
use caligo_model::ipc_v3::{CoreToBridgeMsg, CAPABILITY_QUERY_REVISION};

pub const QUERY_CAPACITY: usize = 32;
const MAX_CAPABILITIES: usize = 9;
const MAX_REASON_BYTES: usize = 4096;

#[derive(Default)]
struct State {
    epoch: u64,
    scope: Option<QueryScope>,
    capabilities: HashSet<QueryCapability>,
    send_private_text: bool,
    send_group_text: bool,
    send_private_voice: bool,
    pending: HashMap<String, Pending>,
    stale_results: u64,
}

struct Pending {
    scope: QueryScope,
    query: HostQuery,
    deadline: Instant,
    tx: mpsc::SyncSender<QueryOutcome>,
}

#[derive(Default)]
pub struct CapabilityBroker {
    state: Mutex<State>,
}

impl CapabilityBroker {
    pub(crate) fn reset(&self, epoch: u64) {
        let mut state = self.state.lock().unwrap();
        Self::drain(&mut state);
        state.epoch = epoch;
        state.scope = None;
        state.capabilities.clear();
        state.send_private_text = false;
        state.send_group_text = false;
        state.send_private_voice = false;
    }

    pub(crate) fn bind(&self, scope: QueryScope) {
        let mut state = self.state.lock().unwrap();
        if scope.connection_epoch == state.epoch && scope.validate().is_ok() {
            state.scope = Some(scope);
            state.capabilities.clear();
            state.send_private_text = false;
            state.send_group_text = false;
            state.send_private_voice = false;
        }
    }

    pub(crate) fn advertise_send(
        &self,
        epoch: u64,
        scope: &QueryScope,
        private_text: bool,
        group_text: bool,
        private_voice: bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if epoch != state.epoch || state.scope.as_ref() != Some(scope) || scope.validate().is_err()
        {
            return false;
        }
        state.send_private_text = private_text;
        state.send_group_text = group_text;
        state.send_private_voice = private_voice;
        true
    }

    pub(crate) fn supports_private_text(&self, scope: &QueryScope) -> bool {
        let state = self.state.lock().unwrap();
        state.scope.as_ref() == Some(scope) && state.send_private_text
    }

    pub(crate) fn supports_private_voice(&self, scope: &QueryScope) -> bool {
        let state = self.state.lock().unwrap();
        state.scope.as_ref() == Some(scope) && state.send_private_voice
    }

    pub(crate) fn advertise(
        &self,
        epoch: u64,
        revision: u32,
        scope: &QueryScope,
        capabilities: Vec<QueryCapability>,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if epoch != state.epoch || state.scope.as_ref() != Some(scope) {
            return false;
        }
        let unique: HashSet<_> = capabilities.iter().copied().collect();
        if revision != CAPABILITY_QUERY_REVISION
            || capabilities.len() > MAX_CAPABILITIES
            || unique.len() != capabilities.len()
        {
            state.capabilities.clear();
            Self::drain(&mut state);
            return false;
        }
        state.capabilities = unique;
        true
    }

    pub(crate) fn begin<'a>(
        &'a self,
        request_id: String,
        scope: QueryScope,
        query: HostQuery,
        timeout: Duration,
        dispatch: &Mutex<Vec<CoreToBridgeMsg>>,
    ) -> Result<QueryTicket<'a>, QueryOutcome> {
        if query.validate().is_err() || scope.validate().is_err() {
            return Err(QueryOutcome::Rejected {
                code: -1,
                reason: "invalid read-only query".into(),
            });
        }
        let timeout = timeout.min(Duration::from_millis(MAX_QUERY_TIMEOUT_MS));
        if timeout.is_zero() {
            return Err(QueryOutcome::Timeout);
        }
        let mut state = self.state.lock().unwrap();
        if state.scope.as_ref() != Some(&scope) {
            return Err(QueryOutcome::Disconnected);
        }
        if !state.capabilities.contains(&query.capability()) {
            return Err(QueryOutcome::Unsupported);
        }
        if state.pending.len() >= QUERY_CAPACITY {
            return Err(QueryOutcome::NotReady);
        }
        if state.pending.contains_key(&request_id) {
            return Err(QueryOutcome::NotReady);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let deadline = Instant::now() + timeout;
        state.pending.insert(
            request_id.clone(),
            Pending {
                scope: scope.clone(),
                query: query.clone(),
                deadline,
                tx,
            },
        );
        // Registration precedes publication, even for an immediate host reply.
        let mut queue = dispatch.lock().unwrap();
        queue.retain(|message| match message {
            CoreToBridgeMsg::Query { request_id, .. } => state.pending.contains_key(request_id),
            _ => true,
        });
        queue.push(CoreToBridgeMsg::Query {
            request_id: request_id.clone(),
            scope,
            timeout_ms: timeout.as_millis().max(1) as u64,
            query,
        });
        Ok(QueryTicket {
            broker: self,
            request_id,
            deadline,
            rx,
        })
    }

    /// Called after releasing dispatch_out. Expired/cancelled/old-epoch entries
    /// never reach a new connection; remaining time excludes queue residence.
    pub(crate) fn dispatch_timeout(
        &self,
        epoch: u64,
        request_id: &str,
        scope: &QueryScope,
    ) -> Option<u64> {
        let state = self.state.lock().unwrap();
        let pending = state.pending.get(request_id)?;
        if epoch != state.epoch || &pending.scope != scope || state.scope.as_ref() != Some(scope) {
            return None;
        }
        let remaining = pending.deadline.checked_duration_since(Instant::now())?;
        Some(remaining.as_millis().max(1) as u64)
    }

    pub(crate) fn complete(
        &self,
        epoch: u64,
        request_id: &str,
        scope: &QueryScope,
        outcome: QueryOutcome,
    ) {
        let mut state = self.state.lock().unwrap();
        let matches = epoch == state.epoch
            && state.scope.as_ref() == Some(scope)
            && state
                .pending
                .get(request_id)
                .is_some_and(|p| &p.scope == scope);
        if !matches {
            state.stale_results = state.stale_results.saturating_add(1);
            return;
        }
        let pending = state.pending.remove(request_id).unwrap();
        let outcome = if Instant::now() >= pending.deadline {
            QueryOutcome::Timeout
        } else {
            match outcome {
                QueryOutcome::Success(data)
                    if data.validate_for(&pending.query, scope).is_err() =>
                {
                    QueryOutcome::InvalidResponse
                }
                QueryOutcome::Rejected { ref reason, .. } if reason.len() > MAX_REASON_BYTES => {
                    QueryOutcome::InvalidResponse
                }
                other => other,
            }
        };
        let _ = pending.tx.try_send(outcome);
    }

    pub(crate) fn disconnect(&self, epoch: u64) {
        let mut state = self.state.lock().unwrap();
        if state.epoch == epoch {
            state.scope = None;
            state.capabilities.clear();
            state.send_private_text = false;
            state.send_group_text = false;
            state.send_private_voice = false;
            Self::drain(&mut state);
        }
    }

    fn drain(state: &mut State) {
        for (_, pending) in state.pending.drain() {
            let _ = pending.tx.try_send(QueryOutcome::Disconnected);
        }
    }

    pub fn pending_count(&self) -> usize {
        self.state.lock().unwrap().pending.len()
    }
    pub fn stale_result_count(&self) -> u64 {
        self.state.lock().unwrap().stale_results
    }
}

/// Dropping an abandoned waiter removes its correlation; late replies are audit only.
pub(crate) struct QueryTicket<'a> {
    broker: &'a CapabilityBroker,
    request_id: String,
    deadline: Instant,
    rx: mpsc::Receiver<QueryOutcome>,
}

impl QueryTicket<'_> {
    pub(crate) fn wait(self, stop: &AtomicBool, cancel: &AtomicBool) -> QueryOutcome {
        loop {
            if stop.load(Ordering::Relaxed) {
                return QueryOutcome::Disconnected;
            }
            if cancel.load(Ordering::Relaxed) {
                return QueryOutcome::Rejected {
                    code: -2,
                    reason: "query cancelled".into(),
                };
            }
            let Some(remaining) = self.deadline.checked_duration_since(Instant::now()) else {
                return QueryOutcome::Timeout;
            };
            match self
                .rx
                .recv_timeout(remaining.min(Duration::from_millis(20)))
            {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return QueryOutcome::Disconnected,
            }
        }
    }
}

impl Drop for QueryTicket<'_> {
    fn drop(&mut self) {
        self.broker
            .state
            .lock()
            .unwrap()
            .pending
            .remove(&self.request_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (CapabilityBroker, QueryScope, Mutex<Vec<CoreToBridgeMsg>>) {
        let broker = CapabilityBroker::default();
        let scope = QueryScope {
            account: "10001".into(),
            host_nonce: 1,
            session_generation: 1,
            connection_epoch: 2,
        };
        broker.reset(2);
        broker.bind(scope.clone());
        assert!(broker.advertise(2, 1, &scope, vec![QueryCapability::FriendList]));
        (broker, scope, Mutex::new(vec![]))
    }
    #[test]
    fn capacity_drop_and_early_reply_are_bounded() {
        let (broker, scope, queue) = setup();
        let tickets: Vec<_> = (0..QUERY_CAPACITY)
            .map(|i| {
                broker
                    .begin(
                        i.to_string(),
                        scope.clone(),
                        HostQuery::FriendList,
                        Duration::from_secs(60),
                        &queue,
                    )
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            broker.begin(
                "full".into(),
                scope.clone(),
                HostQuery::FriendList,
                Duration::from_secs(1),
                &queue
            ),
            Err(QueryOutcome::NotReady)
        ));
        broker.complete(2, "0", &scope, QueryOutcome::Unsupported);
        assert_eq!(
            tickets
                .into_iter()
                .next()
                .unwrap()
                .wait(&AtomicBool::new(false), &AtomicBool::new(false)),
            QueryOutcome::Unsupported
        );
        assert_eq!(broker.pending_count(), 0);
        let ticket = broker
            .begin(
                "fresh".into(),
                scope,
                HostQuery::FriendList,
                Duration::from_secs(1),
                &queue,
            )
            .unwrap();
        assert_eq!(queue.lock().unwrap().len(), 1);
        drop(ticket);
        assert_eq!(broker.pending_count(), 0);
    }

    #[test]
    fn dropped_and_expired_waiters_never_dispatch() {
        let (broker, scope, queue) = setup();
        let ticket = broker
            .begin(
                "drop".into(),
                scope.clone(),
                HostQuery::FriendList,
                Duration::from_secs(1),
                &queue,
            )
            .unwrap();
        assert!(broker.dispatch_timeout(2, "drop", &scope).is_some());
        drop(ticket);
        assert!(broker.dispatch_timeout(2, "drop", &scope).is_none());
        broker.complete(2, "drop", &scope, QueryOutcome::Unsupported);
        assert_eq!(broker.stale_result_count(), 1);
        let ticket = broker
            .begin(
                "expired".into(),
                scope.clone(),
                HostQuery::FriendList,
                Duration::from_millis(1),
                &queue,
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(broker.dispatch_timeout(2, "expired", &scope).is_none());
        assert_eq!(
            ticket.wait(&AtomicBool::new(false), &AtomicBool::new(false)),
            QueryOutcome::Timeout
        );
        assert_eq!(broker.pending_count(), 0);
    }

    #[test]
    fn reconnect_revokes_old_epoch_and_unknown_revision_revokes_support() {
        let (broker, scope, queue) = setup();
        let ticket = broker
            .begin(
                "old".into(),
                scope.clone(),
                HostQuery::FriendList,
                Duration::from_secs(60),
                &queue,
            )
            .unwrap();
        assert!(broker.dispatch_timeout(2, "old", &scope).unwrap() <= MAX_QUERY_TIMEOUT_MS);
        broker.reset(3);
        assert_eq!(
            ticket.wait(&AtomicBool::new(false), &AtomicBool::new(false)),
            QueryOutcome::Disconnected
        );
        let mut new_scope = scope.clone();
        new_scope.connection_epoch = 3;
        broker.bind(new_scope.clone());
        assert!(!broker.advertise(2, 1, &scope, vec![QueryCapability::FriendList]));
        assert!(matches!(
            broker.begin(
                "new".into(),
                new_scope.clone(),
                HostQuery::FriendList,
                Duration::from_secs(1),
                &queue
            ),
            Err(QueryOutcome::Unsupported)
        ));
        assert!(broker.advertise(3, 1, &new_scope, vec![QueryCapability::FriendList]));
        assert!(!broker.advertise(3, 99, &new_scope, vec![QueryCapability::FriendList]));
        assert!(matches!(
            broker.begin(
                "newer".into(),
                new_scope,
                HostQuery::FriendList,
                Duration::from_secs(1),
                &queue
            ),
            Err(QueryOutcome::Unsupported)
        ));
    }
}

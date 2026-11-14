//! Synthetic owner contract: revocation survives a rebind before owner drain.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, ThreadId};

use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};
use caligo_model::capability::{
    HostQuery, LoginInfo, QueryCapability, QueryData, QueryOutcome, QueryScope,
};

struct QueryHost {
    owner: ThreadId,
    context: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<(String, &'static str)>>>,
}
impl HostAdapter for QueryHost {
    fn owner_thread_id(&self) -> u64 {
        1
    }
    fn current_thread_id(&self) -> u64 {
        if thread::current().id() == self.owner {
            1
        } else {
            2
        }
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        self.context.load(Ordering::Acquire)
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        assert_eq!(thread::current().id(), self.owner);
        match op {
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
            _ => panic!("query fixture must not send or register extra listeners"),
        }
    }
    fn query_capabilities(&self) -> &'static [QueryCapability] {
        &[QueryCapability::LoginInfo, QueryCapability::SelfProfile]
    }
    fn query(
        &mut self,
        id: &str,
        scope: &QueryScope,
        query: &HostQuery,
        _: u64,
        poll: bool,
    ) -> Option<QueryOutcome> {
        assert_eq!(thread::current().id(), self.owner);
        self.calls
            .lock()
            .unwrap()
            .push((id.into(), if poll { "poll" } else { "start" }));
        if !poll {
            return None;
        }
        let info = LoginInfo {
            user_id: scope.account.clone(),
            nickname: "Synthetic self".into(),
            online: true,
        };
        Some(QueryOutcome::Success(match query {
            HostQuery::LoginInfo => QueryData::LoginInfo(info),
            HostQuery::SelfProfile => QueryData::SelfProfile(info),
            _ => panic!("fixture only queries self"),
        }))
    }
    fn cancel_query(&mut self, id: &str, _: &QueryScope) {
        assert_eq!(thread::current().id(), self.owner);
        self.calls.lock().unwrap().push((id.into(), "cancel"));
    }
}
fn scope() -> QueryScope {
    QueryScope {
        account: "10001".into(),
        host_nonce: 1,
        session_generation: 1,
        connection_epoch: 2,
    }
}
fn fixture() -> (
    Arc<Resident<QueryHost>>,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<(String, &'static str)>>>,
) {
    let context = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let resident = Arc::new(Resident::new(
        QueryHost {
            owner: thread::current().id(),
            context: context.clone(),
            calls: calls.clone(),
        },
        ResidentLimits::default(),
    ));
    resident.bootstrap().unwrap();
    resident.set_bridge_scope(Some(scope()));
    (resident, context, calls)
}

#[test]
fn same_scope_rebind_cannot_resume_revoked_queries() {
    for query in [HostQuery::LoginInfo, HostQuery::SelfProfile] {
        let (resident, _, calls) = fixture();
        resident
            .submit_query("old".into(), scope(), 5000, query.clone())
            .unwrap();
        assert_eq!(resident.drain_queries(), 1); // Old request is already in flight.
        let worker = resident.clone();
        thread::spawn(move || {
            worker.set_bridge_scope(None);
            // A restarted core can assign the same numeric epoch before owner runs.
            worker.invalidate_queries_except(&scope());
            worker.set_bridge_scope(Some(scope()));
        })
        .join()
        .unwrap();
        assert_eq!(resident.drain_queries(), 1);
        assert_eq!(
            *calls.lock().unwrap(),
            [("old".into(), "start"), ("old".into(), "cancel")]
        );
        assert_eq!(
            resident.take_query_results(),
            [("old".into(), scope(), QueryOutcome::Disconnected)]
        );
        // Once cancellation has run, a new request under the rebound scope can work.
        resident
            .submit_query("new".into(), scope(), 5000, query)
            .unwrap();
        assert_eq!(resident.drain_queries(), 1);
        assert_eq!(resident.drain_queries(), 1);
        let results = resident.take_query_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "new");
        assert!(matches!(results[0].2, QueryOutcome::Success(_)));
    }
}

#[test]
fn queued_revoked_query_never_starts_after_same_scope_rebind() {
    let (resident, _, calls) = fixture();
    resident
        .submit_query("queued".into(), scope(), 5000, HostQuery::SelfProfile)
        .unwrap();
    resident.set_bridge_scope(None);
    resident.set_bridge_scope(Some(scope()));
    resident.drain_queries();
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        resident.take_query_results(),
        [("queued".into(), scope(), QueryOutcome::Disconnected)]
    );
}

#[test]
fn unchanged_scope_does_not_cancel_a_live_query() {
    let (resident, _, calls) = fixture();
    resident
        .submit_query("live".into(), scope(), 5000, HostQuery::LoginInfo)
        .unwrap();
    resident.drain_queries();
    resident.set_bridge_scope(Some(scope()));
    resident.invalidate_queries_except(&scope());
    resident.drain_queries();
    assert_eq!(
        *calls.lock().unwrap(),
        [("live".into(), "start"), ("live".into(), "poll")]
    );
    assert!(matches!(
        resident.take_query_results()[0].2,
        QueryOutcome::Success(_)
    ));
}

#[test]
fn cancellation_waits_for_owner_context_and_is_reported_once() {
    let (resident, context, calls) = fixture();
    resident
        .submit_query("old".into(), scope(), 5000, HostQuery::SelfProfile)
        .unwrap();
    resident.drain_queries();
    context.store(false, Ordering::Release);
    let mut next = scope();
    next.connection_epoch += 1;
    resident.set_bridge_scope(None);
    resident.invalidate_queries_except(&next);
    resident.set_bridge_scope(Some(next.clone()));
    resident.invalidate_queries_except(&next);
    assert_eq!(resident.drain_queries(), 1);
    assert_eq!(*calls.lock().unwrap(), [("old".into(), "start")]);
    assert_eq!(
        resident.take_query_results(),
        [("old".into(), scope(), QueryOutcome::Disconnected)]
    );
    // Repeated revocation while cleanup is pending must not duplicate the result.
    resident.set_bridge_scope(None);
    resident.set_bridge_scope(Some(next));
    assert_eq!(resident.drain_queries(), 1);
    assert!(resident.take_query_results().is_empty());
    context.store(true, Ordering::Release);
    assert_eq!(resident.drain_queries(), 1);
    assert_eq!(
        *calls.lock().unwrap(),
        [("old".into(), "start"), ("old".into(), "cancel")]
    );
    assert_eq!(resident.drain_queries(), 0);
}

#[test]
fn scope_change_stays_revoked_when_original_scope_returns() {
    let (resident, _, calls) = fixture();
    resident
        .submit_query("old".into(), scope(), 5000, HostQuery::LoginInfo)
        .unwrap();
    resident.drain_queries();
    let mut other = scope();
    other.connection_epoch += 1;
    resident.set_bridge_scope(Some(other));
    resident.set_bridge_scope(Some(scope()));
    resident.drain_queries();
    assert_eq!(
        *calls.lock().unwrap(),
        [("old".into(), "start"), ("old".into(), "cancel")]
    );
    assert_eq!(
        resident.take_query_results(),
        [("old".into(), scope(), QueryOutcome::Disconnected)]
    );
}

use caligo_bridge::host_adapter::{HostAdapter,HostError,HostOp,HostOpResult};
use caligo_bridge::resident::{OwnedRequest,Resident,ResidentLimits,ResidentNotice,SendOutcome,SubmitVerdict};

struct FiredHost;
impl HostAdapter for FiredHost {
    fn owner_thread_id(&self)->u64{1}
    fn current_thread_id(&self)->u64{1}
    fn env_valid(&self)->bool{true}
    fn current_context_ok(&self)->bool{true}
    fn native_op(&mut self,op:HostOp)->Result<HostOpResult,HostError>{
        match op {
            HostOp::ListenerAdd=>Ok(HostOpResult::ListenerDeferred),
            HostOp::ListenerRemove{..}=>Ok(HostOpResult::ListenerRemoved),
            HostOp::SendText{..}=>Ok(HostOpResult::SendFired{mid:"local".into()}),
            _=>panic!("unexpected host operation"),
        }
    }
}
#[test]
fn duplicate_unconfirmed_result_does_not_enqueue_another_notice_and_strong_id_can_finish() {
    let resident=Resident::new(FiredHost,ResidentLimits::default());resident.bootstrap().unwrap();
    assert_eq!(resident.submit(OwnedRequest::SendText{request_id:"request-1".into(),wire_seq:1,
        timeout_ms:5000,chat_type:2,peer_uid:"123".into(),text:"sample".into()}),SubmitVerdict::Accepted);
    assert_eq!(resident.drain().unwrap(),1);resident.take_results();
    let empty=[("request-1".to_string(),String::new())];
    resident.complete_sends(&empty,&[],&[]);
    assert_eq!(resident.take_results(),vec![ResidentNotice::Outcome(SendOutcome::AcceptedUnconfirmed{request_id:"request-1".into()})]);
    resident.complete_sends(&empty,&[],&[]);
    assert!(resident.take_results().is_empty(),"duplicate unconfirmed result must not enqueue again");
    resident.complete_sends(&[("request-1".into(),"native-1".into())],&[],&[]);
    assert_eq!(resident.take_results(),vec![ResidentNotice::Outcome(SendOutcome::Success{request_id:"request-1".into(),native_id:"native-1".into()})]);
    resident.complete_sends(&[("request-1".into(),"native-1".into())],&[],&[]);
    assert!(resident.take_results().is_empty());
}

use caligo_bridge::host_adapter::{HostAdapter,HostError,HostOp,HostOpResult};
use caligo_bridge::resident::{OwnedRequest,Resident,ResidentLimits,SubmitVerdict};
use std::collections::HashSet;
struct FiredHost;
impl HostAdapter for FiredHost {
 fn owner_thread_id(&self)->u64{1} fn current_thread_id(&self)->u64{1}
 fn env_valid(&self)->bool{true} fn current_context_ok(&self)->bool{true}
 fn native_op(&mut self,op:HostOp)->Result<HostOpResult,HostError>{match op {
  HostOp::ListenerAdd=>Ok(HostOpResult::ListenerDeferred),
  HostOp::SendText{..}=>Ok(HostOpResult::SendFired{mid:"local".into()}),
  _=>panic!("unexpected host operation"),
 }}
}
fn request(id:&str)->OwnedRequest {OwnedRequest::SendText{request_id:id.into(),wire_seq:1,timeout_ms:5000,chat_type:2,peer_uid:"123".into(),text:"sample".into()}}
#[test]
fn only_currently_processed_terminal_results_are_eligible_for_ack() {
 let r=Resident::new(FiredHost,ResidentLimits::default());r.bootstrap().unwrap();
 for key in ["done","rejected","unknown"] {assert_eq!(r.submit(request(key)),SubmitVerdict::Accepted);}
 assert_eq!(r.drain().unwrap(),3);r.take_results();
 let done=[("done".into(),"native".into())];let rejected=[("rejected".into(),"error".into())];
 let unknown=[("unknown".into(),"timeout".into()),("orphan".into(),"timeout".into())];
 let accepted:HashSet<_>=r.complete_sends(&done,&rejected,&unknown).into_iter().collect();
 assert_eq!(accepted,HashSet::from(["done".to_string(),"rejected".to_string(),"unknown".to_string()]));
 assert_eq!(r.take_results().len(),3);
 assert!(r.complete_sends(&done,&rejected,&unknown).is_empty(),"completed or unknown requests must not accept different duplicate outcomes");
 assert!(r.take_results().is_empty());
}
#[test]
fn already_accepted_empty_identity_can_be_acknowledged_without_duplicate_notice() {
 let r=Resident::new(FiredHost,ResidentLimits::default());r.bootstrap().unwrap();assert_eq!(r.submit(request("pending")),SubmitVerdict::Accepted);
 r.drain().unwrap();r.take_results();let empty=[("pending".into(),String::new())];
 assert_eq!(r.complete_sends(&empty,&[],&[]),vec!["pending".to_string()]);assert_eq!(r.take_results().len(),1);
 assert_eq!(r.complete_sends(&empty,&[],&[]),vec!["pending".to_string()]);assert!(r.take_results().is_empty());
 assert_eq!(r.complete_sends(&[("pending".into(),"native".into())],&[],&[]),vec!["pending".to_string()]);assert_eq!(r.take_results().len(),1);
}

#[test]
fn expired_unknown_does_not_accept_a_late_identity_for_acknowledgement() {
 let r=Resident::new(FiredHost,ResidentLimits::default());r.bootstrap().unwrap();assert_eq!(r.submit(request("expired")),SubmitVerdict::Accepted);
 r.drain().unwrap();r.take_results();assert_eq!(r.complete_sends(&[],&[],&[("expired".into(),"timeout".into())]),vec!["expired".to_string()]);r.take_results();
 assert_eq!(r.expire_late_receipts(std::time::Instant::now()+std::time::Duration::from_secs(3600)),1);
 assert!(r.complete_sends(&[("expired".into(),"native".into())],&[],&[]).is_empty());
 assert!(r.take_results().is_empty());
}

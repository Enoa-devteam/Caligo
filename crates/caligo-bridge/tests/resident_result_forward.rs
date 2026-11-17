use caligo_bridge::host_adapter::{HostAdapter,HostError,HostOp,HostOpResult};
use caligo_bridge::resident::{OwnedRequest,Resident,ResidentLimits,ResidentNotice,SendOutcome,SubmitVerdict,ResultHandoffGate};
struct Host;
impl HostAdapter for Host {
 fn owner_thread_id(&self)->u64{1} fn current_thread_id(&self)->u64{1}
 fn env_valid(&self)->bool{true} fn current_context_ok(&self)->bool{true}
 fn native_op(&mut self,op:HostOp)->Result<HostOpResult,HostError>{match op {
  HostOp::ListenerAdd=>Ok(HostOpResult::ListenerDeferred),
  HostOp::SendText{..}=>Ok(HostOpResult::Sent{native_id:Some("native".into())}),
  _=>panic!("unexpected host operation"),
 }}
}
fn resident()->Resident<Host>{
 let r=Resident::new(Host,ResidentLimits::default());r.bootstrap().unwrap();
 assert_eq!(r.submit(OwnedRequest::SendText{request_id:"request".into(),wire_seq:1,timeout_ms:5000,chat_type:2,peer_uid:"123".into(),text:"fixture".into()}),SubmitVerdict::Accepted);
 assert_eq!(r.drain().unwrap(),1);r
}
#[test]
fn closed_worker_channel_must_not_discard_notices() {
 let r=resident();let (tx,rx)=std::sync::mpsc::channel();drop(rx);
 assert_eq!(r.forward_results(|notice|tx.send(notice).map_err(|e|e.0)),Err(0));
 let notices=r.take_results();assert_eq!(notices.len(),2);
 assert!(matches!(&notices[0],ResidentNotice::NativeStarted{request_id} if request_id=="request"));
 assert!(matches!(&notices[1],ResidentNotice::Outcome(SendOutcome::Success{request_id,..}) if request_id=="request"));
}

#[test]
fn partial_handoff_restores_unsent_suffix_in_fifo_order() {
 let r=resident();let mut delivered=Vec::new();let mut attempt=0;
 assert_eq!(r.forward_results(|notice|{
  attempt+=1;if attempt==1 {delivered.push(notice);Ok(())} else {
   assert_eq!(r.submit(OwnedRequest::SendText{request_id:"later".into(),wire_seq:2,timeout_ms:5000,chat_type:2,peer_uid:"123".into(),text:"fixture".into()}),SubmitVerdict::Accepted);
   assert_eq!(r.drain().unwrap(),1);Err(notice)
  }
 }),Err(1));
 assert_eq!(delivered.len(),1);assert!(matches!(&delivered[0],ResidentNotice::NativeStarted{..}));
 let remaining=r.take_results();assert_eq!(remaining.len(),3);
 assert!(matches!(&remaining[0],ResidentNotice::Outcome(SendOutcome::Success{request_id,..}) if request_id=="request"));
 assert!(matches!(&remaining[1],ResidentNotice::NativeStarted{request_id} if request_id=="later"));
 assert!(matches!(&remaining[2],ResidentNotice::Outcome(SendOutcome::Success{request_id,..}) if request_id=="later"));
}
#[test]
fn live_worker_receives_ordered_notices_once() {
 let r=resident();let (tx,rx)=std::sync::mpsc::channel();
 assert_eq!(r.forward_results(|notice|tx.send(notice).map_err(|e|e.0)),Ok(2));
 assert!(matches!(rx.try_recv().unwrap(),ResidentNotice::NativeStarted{..}));
 assert!(matches!(rx.try_recv().unwrap(),ResidentNotice::Outcome(SendOutcome::Success{..})));
 assert!(rx.try_recv().is_err());assert!(r.take_results().is_empty());
}

#[test]
fn disconnected_worker_latches_and_cannot_reopen_on_later_ticks() {
 let r=resident();let gate=ResultHandoffGate::default();let (dead,rx)=std::sync::mpsc::channel();drop(rx);
 assert_eq!(gate.forward(&r,&dead),Err(0));assert!(gate.is_closed());
 let (new_sender,new_receiver)=std::sync::mpsc::channel();
 assert_eq!(gate.forward(&r,&new_sender),Err(0));assert!(new_receiver.try_recv().is_err());
 assert_eq!(r.take_results().len(),2,"latched gate must retain both notices");
}

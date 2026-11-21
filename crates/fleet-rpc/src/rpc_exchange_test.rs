use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct TestRpc;
impl Rpc for TestRpc {
    type Target = u8;
    type Request = String;
    type Reply = String;
}

struct TestTransport {
    reply_target: u8,
}

impl Transport<TestRpc> for TestTransport {
    async fn exchange(
        &self,
        request: Envelope<u8, String>,
    ) -> Result<Envelope<u8, String>, CallFailure> {
        let mut reply = dispatch::<TestRpc, _, _>(&42, request, |request| async { request })
            .await
            .map_err(|_| CallFailure::NeverReached)?;
        reply.target = self.reply_target;
        Ok(reply)
    }
}

#[test]
fn reject_wrong_target_before_dispatch_and_reject_a_mismatched_reply() {
    // These test futures perform no I/O and are ready at first poll.
    let invoked = AtomicBool::new(false);
    let wrong = dispatch::<TestRpc, _, _>(
        &42,
        Envelope {
            target: 43,
            body: "request".into(),
        },
        |_| async {
            invoked.store(true, Ordering::SeqCst);
            "reply".into()
        },
    );
    let mut wrong = std::pin::pin!(wrong);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        wrong.as_mut().poll(&mut context),
        std::task::Poll::Ready(Err(TargetMismatch))
    );
    assert!(!invoked.load(Ordering::SeqCst));

    for (reply_target, expected) in [
        (42, Ok("request".to_owned())),
        (43, Err(CallFailure::AnswerLost)),
    ] {
        let client = RpcClient::<TestRpc, _>::new(42, TestTransport { reply_target });
        let call = client.exchange_message("request".into());
        let mut call = std::pin::pin!(call);
        assert_eq!(
            call.as_mut().poll(&mut context),
            std::task::Poll::Ready(expected)
        );
    }
}

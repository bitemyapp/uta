use aizome_uta::{
    instance::{Claim, claim_profile_in},
    service::{IgnoreCredentials, Reply, Request, Secret, Service},
};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Backend(Mutex<Vec<bool>>);
impl Service for Backend {
    fn call(&self, request: Request, authenticated: bool) -> Result<Reply, String> {
        assert!(matches!(
            request,
            Request::Resolve { .. } | Request::Search { .. }
        ));
        self.0.lock().unwrap().push(authenticated);
        Ok(Reply::default())
    }
}
#[test]
fn ignored_credentials_never_reach_account_io_even_if_ui_thinks_connected() {
    let backend = Arc::new(Backend::default());
    let service = IgnoreCredentials(backend.clone());
    for request in [
        Request::Status,
        Request::Discover,
        Request::Disconnect,
        Request::NativeSession,
        Request::Connect {
            source: "synthetic".into(),
        },
        Request::ConnectText {
            text: Secret("synthetic".into()),
        },
        Request::Liked,
        Request::Playlists,
        Request::Albums,
        Request::History,
        Request::Rate {
            id: "abcdefghijk".into(),
            like: true,
        },
    ] {
        if let Ok(reply) = service.call(request, true) {
            assert!(!reply.connected);
        }
    }
    assert!(backend.0.lock().unwrap().is_empty());
    service
        .call(
            Request::Resolve {
                id: "abcdefghijk".into(),
            },
            true,
        )
        .unwrap();
    assert_eq!(*backend.0.lock().unwrap(), [false]);
}
#[test]
fn comparison_instances_coexist_and_only_activate_their_own_profile() {
    let dir = tempfile::tempdir().unwrap();
    let Claim::First(auth) = claim_profile_in(dir.path().into(), &[], "default").unwrap() else {
        panic!()
    };
    let Claim::First(anon) = claim_profile_in(dir.path().into(), &[], "anonymous").unwrap() else {
        panic!()
    };
    assert!(matches!(
        claim_profile_in(dir.path().into(), &[], "anonymous").unwrap(),
        Claim::Handed
    ));
    assert!(dir.path().join("aizome-app.uta.sock").exists());
    drop(anon);
    assert!(dir.path().join("aizome-app.uta.sock").exists());
    drop(auth);
}
#[test]
fn native_session_debug_redacts_cookies() {
    let reply: Reply = serde_json::from_str(
        r#"{"credentials":{"cookie":"PRIVATE_SESSION","authuser":"0","expires":123}}"#,
    )
    .unwrap();
    assert!(!format!("{reply:?}").contains("PRIVATE_SESSION"));
}

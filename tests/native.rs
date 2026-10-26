use aizome_kit::prelude::*;
use aizome_uta::{
    app::{Msg, Uta},
    service::Demo,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn native_window_opens_renders_and_closes() {
    if std::env::var_os("UTA_NATIVE_TEST_CHILD").is_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_window_opens_renders_and_closes",
                "--nocapture",
            ])
            .env("UTA_NATIVE_TEST_CHILD", "1")
            .env("SIDESTEP_BACKEND", "null")
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("XDG_CONFIG_HOME", dir.path())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            if Instant::now() >= until {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Native app failed to close");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    struct Smoke(Uta);
    impl Program for Smoke {
        type Msg = Msg;
        fn info(&self) -> AppInfo {
            self.0.info()
        }
        fn launched(&mut self, cx: &mut Cx<Msg>) {
            self.0.launched(cx);
            cx.after(Duration::from_millis(300), Msg::Quit);
        }
        fn update(&mut self, message: Msg, cx: &mut Cx<Msg>) {
            self.0.update(message, cx);
        }
        fn view(&self, window: WindowKey) -> Element<Msg> {
            self.0.view(window)
        }
        fn menus(&self) -> MenuBar<Msg> {
            self.0.menus()
        }
    }
    let app = aizome_kit::try_run(Smoke(Uta::new(Arc::new(Demo), None, true))).unwrap();
    assert_ne!(app.0.window, WindowKey::NONE);
    assert_eq!(app.0.tracks.len(), 6);
}

//! Monitor only a private synthetic bus; never connect this test to the owner's desktop.
use std::{process::Command, time::Duration};

#[test]
fn notification_monitor_contract_on_an_isolated_bus() {
    let mut child = Command::new("dbus-run-session")
        .arg("--")
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "synthetic_notify", "--ignored", "--nocapture"])
        .env("D_HUB_SYNTHETIC_BUS", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("dbus-run-session is required for the notification protocol test");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("private bus test timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "executed by the isolated-bus wrapper only"]
fn synthetic_notify() {
    use d_hub::{
        discord_notifications::{self, Event},
        domain::Provider,
        policy::AccountConfig,
    };
    assert_eq!(std::env::var("D_HUB_SYNTHETIC_BUS").as_deref(), Ok("1"));
    let mut account = AccountConfig::new("synthetic", Provider::Discord);
    account.discord_mode = "notifications".into();
    account.notification_sender = "Fictional Son".into();
    let server = zbus::blocking::Connection::session().unwrap();
    server
        .request_name("org.freedesktop.Notifications")
        .unwrap();
    let receiver = discord_notifications::start(vec![account]).unwrap();
    match receiver.recv_timeout(Duration::from_secs(3)).unwrap() {
        Event::Ready => {}
        Event::Error(e) => panic!("{e}"),
        _ => panic!("expected ready"),
    }
    let sender = zbus::blocking::Connection::session().unwrap();
    for (app, person) in [
        ("OtherApp", "Fictional Son"),
        ("Discord", "Someone Else"),
        ("Discord", "Fictional Son"),
    ] {
        let message = zbus::Message::method_call("/org/freedesktop/Notifications", "Notify")
            .unwrap()
            .destination("org.freedesktop.Notifications")
            .unwrap()
            .interface("org.freedesktop.Notifications")
            .unwrap()
            .build(&(
                app,
                0_u32,
                "",
                person,
                "Fictional message",
                Vec::<String>::new(),
                std::collections::HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                -1_i32,
            ))
            .unwrap();
        sender.send(&message).unwrap();
    }
    match receiver.recv_timeout(Duration::from_secs(3)).unwrap() {
        Event::Notification(n) => {
            assert_eq!(n.summary, "Fictional Son");
            assert_eq!(n.app, "Discord");
            assert_eq!(n.body, "Fictional message");
            assert_eq!(n.event_id.split(':').count(), 4);
        }
        Event::Error(e) => panic!("{e}"),
        _ => panic!("expected matching notification"),
    }
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
}

#![cfg(target_os = "linux")]
use std::io::BufRead;
use std::process::{Command, Stdio};

#[test]
fn probe_child() {
    if std::env::var_os("OPENLESS_PROBE_BUS_TEST").is_none() { return; }
    let status = openless_linux_host::plugin_diagnostics();
    assert_eq!(status["daemonRunning"], false);
    assert_eq!(status["loaded"], false);
    assert_eq!(status["insertionReady"], false);
    assert!(!openless_linux_host::fcitx5_available(), "legacy status probe must not activate Fcitx5 either");
}

#[test]
fn settings_probe_never_activates_an_installed_fcitx_service() {
    let root = std::env::temp_dir().join(format!("openless-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("services")).unwrap();
    let marker = root.join("ACTIVATED");
    std::fs::write(root.join("services/org.fcitx.Fcitx5.service"), format!(
        "[D-BUS Service]\nName=org.fcitx.Fcitx5\nExec=/usr/bin/touch {}\n", marker.display(),
    )).unwrap();
    let config = root.join("bus.conf");
    std::fs::write(&config, format!(r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><servicedir>{}/services</servicedir>
<policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>"#, root.display())).unwrap();
    let mut daemon = Command::new("dbus-daemon").arg("--nofork").arg("--print-address=1")
        .arg(format!("--config-file={}", config.display())).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut address = String::new();
    std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let status = Command::new(std::env::current_exe().unwrap()).args(["--exact", "probe_child", "--nocapture"])
        .env("OPENLESS_PROBE_BUS_TEST", "1").env("DBUS_SESSION_BUS_ADDRESS", address.trim()).status();
    let activated = marker.exists();
    let _ = daemon.kill(); let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(root);
    assert!(status.unwrap().success());
    assert!(!activated, "read-only settings diagnostics auto-started the user's input method");
}

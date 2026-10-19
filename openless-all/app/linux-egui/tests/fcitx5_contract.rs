#![cfg(target_os = "linux")]

use openless_core::BackendErrorCode;
use openless_linux_egui::{
    fcitx5_available, fcitx5_commit_text, fcitx5_selection_text, set_fcitx5_hotkeys,
    set_fcitx5_less_computer_hotkey_raw, Fcitx5HotkeyListener,
};

/// Exercise the real fcitx5 DBus object and listener lifecycle.  Key event
/// ordering still needs a desktop runner with an actual focused input context.
#[test]
#[ignore = "requires a running fcitx5 DBus service"]
fn fcitx5_dbus_methods_and_listener_have_stable_platform_semantics() {
    assert!(fcitx5_available(), "fcitx5 service should answer DBus Ping");
    // 这个契约测试只验证 DBus 方法可用：**必须清空**而不是注册真实按键。
    // 曾经这里注册 Shift_L + 'A'，在开发机上跑 --ignored 会真的让插件吞掉
    // Shift/字母输入（用户踩过这个坑），所以现在只发清零请求。
    set_fcitx5_hotkeys(Vec::new()).expect("clear the legacy fcitx5 hotkey list");
    set_fcitx5_less_computer_hotkey_raw(0, 0).expect("clear the Less Computer hotkey");

    let listener = Fcitx5HotkeyListener::start().expect("start fcitx5 hotkey listener");
    assert!(listener.take_error().is_none());

    for (member, is_press) in [
        ("DictationKeyEvent", true),
        ("DictationKeyEvent", false),
        ("DictationKeyCombined", true),
        ("LessComputerKeyEvent", true),
        ("LessComputerKeyEvent", false),
        ("LessComputerKeyCombined", true),
        ("TranslationModifierEvent", true),
    ] {
        let status = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/openless",
                &format!("org.fcitx.Fcitx.OpenLess1.{member}"),
                "uint32:65",
                "uint32:0",
                &format!("boolean:{is_press}"),
            ])
            .status()
            .expect("emit fcitx5 contract signal");
        assert!(status.success());
    }
    let status = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--type=signal",
            "/openless",
            "org.fcitx.Fcitx.OpenLess1.DictationEnterStopRequested",
            "string:00000000-0000-0000-0000-000000000001",
        ])
        .status()
        .expect("emit fcitx5 Enter-stop signal");
    assert!(status.success());
    let mut events = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        listener.drain(|event| events.push(event));
        assert!(listener.take_error().is_none());
        if events.len() >= 8 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "fcitx5 signals did not arrive");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(events.len(), 8);
    assert!(matches!(
        events[0],
        openless_linux_egui::LinuxHotkeyEvent::DictationPressed { .. }
    ));
    assert!(matches!(
        events[1],
        openless_linux_egui::LinuxHotkeyEvent::DictationReleased { .. }
    ));
    assert!(matches!(
        events[2],
        openless_linux_egui::LinuxHotkeyEvent::DictationCombined { .. }
    ));
    assert!(matches!(
        events[3],
        openless_linux_egui::LinuxHotkeyEvent::LessComputerPressed { .. }
    ));
    assert!(matches!(
        events[4],
        openless_linux_egui::LinuxHotkeyEvent::LessComputerReleased { .. }
    ));
    assert!(matches!(
        events[5],
        openless_linux_egui::LinuxHotkeyEvent::LessComputerCombined { .. }
    ));
    assert!(matches!(
        events[6],
        openless_linux_egui::LinuxHotkeyEvent::TranslationPressed { .. }
    ));
    assert!(matches!(
        events[7],
        openless_linux_egui::LinuxHotkeyEvent::DictationEnterStopRequested { .. }
    ));
    drop(listener);

    match fcitx5_selection_text() {
        Ok(_) => {}
        Err(error) => assert!(
            matches!(
                error.code,
                BackendErrorCode::Platform | BackendErrorCode::Unsupported
            ),
            "unexpected fcitx5 selection error: {error:?}"
        ),
    }
    if let Err(error) = fcitx5_commit_text("openless fcitx5 contract") {
        assert!(
            matches!(
                error.code,
                BackendErrorCode::Platform | BackendErrorCode::Unsupported
            ),
            "unexpected fcitx5 commit error: {error:?}"
        );
    }
}

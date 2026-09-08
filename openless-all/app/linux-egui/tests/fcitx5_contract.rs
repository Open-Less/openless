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
    set_fcitx5_hotkeys(vec!["Shift_L".to_string()]).expect("set fcitx5 hotkey");
    set_fcitx5_less_computer_hotkey_raw(65, 0).expect("set Less Computer hotkey");

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
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mut events = Vec::new();
    assert_eq!(listener.drain(|event| events.push(event)), 7);
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
        openless_linux_egui::LinuxHotkeyEvent::TranslationPressed
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

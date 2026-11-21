fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        // LaunchServices needs the helper's agent identity before AppKit starts.
        println!(
            "cargo:rustc-link-arg-bin=openless-native-capsule=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}

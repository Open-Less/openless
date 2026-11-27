//! Native read-only device facts. No shell, automation script or input events.

pub(super) fn architecture() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        let mut arm64: i32 = 0;
        let mut size = std::mem::size_of_val(&arm64);
        if unsafe {
            libc::sysctlbyname(
                c"hw.optional.arm64".as_ptr(),
                (&mut arm64 as *mut i32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && arm64 == 1
        {
            return "aarch64";
        }
    }
    std::env::consts::ARCH
}

#[cfg(target_os = "macos")]
pub(super) fn system_version() -> Option<String> {
    let mut value = [0u8; 128];
    let mut size = value.len();
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            value.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && size > 1 && size <= value.len())
        .then(|| String::from_utf8_lossy(&value[..size - 1]).into_owned())
}

#[cfg(target_os = "windows")]
pub(super) fn system_version() -> Option<String> {
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(version: *mut Version) -> i32;
    }
    let mut version = Version {
        size: std::mem::size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    (unsafe { RtlGetVersion(&mut version) } == 0)
        .then(|| format!("{}.{}.{}", version.major, version.minor, version.build))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn system_version() -> Option<String> {
    None
}

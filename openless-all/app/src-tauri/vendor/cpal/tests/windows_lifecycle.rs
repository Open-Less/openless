//! Each enumeration scenario runs in a fresh process so another test cannot
//! initialize COM/CPAL first and conceal a stale process-global enumerator.
#![cfg(target_os = "windows")]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER};

fn isolated(name: &str, scenario: impl FnOnce()) {
    const CHILD: &str = "CPAL_OPENLESS_LIFECYCLE_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        scenario();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--include-ignored"])
        .env(CHILD, name)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{name}: child exited with {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name}: worker cleanup or COM call hung");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn enumerate() {
    let host = cpal::default_host();
    // Empty device lists are valid: obtaining the enumerator and collection is
    // enough to exercise their COM apartment and destruction even in CI.
    for device in host
        .devices()
        .expect("WASAPI enumeration must return normally")
    {
        let _ = device.name();
    }
    if let Some(device) = host.default_input_device() {
        let _ = device.name();
    }
    if let Some(device) = host.default_output_device() {
        let _ = device.name();
    }
}

#[test]
fn enumeration_survives_first_thread_exit() {
    isolated("enumeration_survives_first_thread_exit", || {
        for _ in 0..32 {
            thread::spawn(enumerate).join().unwrap();
        }
        enumerate();
    });
}

#[test]
fn enumeration_is_independent_across_threads() {
    isolated("enumeration_is_independent_across_threads", || {
        // Initialize and destroy the first apartment before concurrent callers.
        thread::spawn(enumerate).join().unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    for _ in 0..16 {
                        enumerate();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
}

#[test]
fn mmdevice_handles_cannot_cross_threads() {
    // If a type implements Send/Sync both impls apply and inference is ambiguous,
    // making this test fail to compile. Check the public dynamic wrappers too.
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    let _ = <cpal::Device as AmbiguousIfSend<_>>::check;
    let _ = <cpal::Device as AmbiguousIfSync<_>>::check;
    let _ = <cpal::Devices as AmbiguousIfSend<_>>::check;
    let _ = <cpal::Devices as AmbiguousIfSync<_>>::check;
}

fn input_stream<T: cpal::SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    panic_in_callback: bool,
) -> (cpal::Stream, mpsc::Receiver<thread::ThreadId>) {
    let (tx, rx) = mpsc::channel();
    let creator = thread::current().id();
    let stream = device
        .build_input_stream::<T, _, _>(
            config,
            move |_, _| {
                let current = thread::current().id();
                assert_ne!(current, creator, "callback should run on its owning worker");
                let mut apartment = APTTYPE::default();
                let mut qualifier = APTTYPEQUALIFIER::default();
                unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }
                    .expect("the WASAPI callback worker must have COM initialized");
                let _ = tx.send(current);
                if panic_in_callback {
                    panic!("intentional callback panic to verify stream cleanup");
                }
            },
            |error| panic!("hardware stream error: {error}"),
            None,
        )
        .expect("hardware capture must initialize successfully");
    (stream, rx)
}

fn capture_once(panic_in_callback: bool, explicit_pause: bool) {
    let device = cpal::default_host()
        .default_input_device()
        .expect("this explicitly requested hardware test requires an input device");
    let supported = device.default_input_config().unwrap();
    let config = supported.config();
    let (stream, callbacks) = match supported.sample_format() {
        cpal::SampleFormat::F32 => input_stream::<f32>(&device, &config, panic_in_callback),
        cpal::SampleFormat::I16 => input_stream::<i16>(&device, &config, panic_in_callback),
        cpal::SampleFormat::U16 => input_stream::<u16>(&device, &config, panic_in_callback),
        other => panic!("hardware test needs a supported fixture format; got {other:?}"),
    };
    stream.play().unwrap();
    callbacks
        .recv_timeout(Duration::from_secs(5))
        .expect("capture callback did not arrive");
    if explicit_pause && !panic_in_callback {
        stream.pause().unwrap();
    }
    // Includes direct cancellation and a worker that may already have unwound.
    // Destruction must join it, stop the client and release its services/events.
    drop(stream);
}

#[test]
#[ignore = "requires a real Windows input device and microphone permission; no-device is a failure"]
fn hardware_capture_stop_cancel_and_callback_panic() {
    isolated("hardware_capture_stop_cancel_and_callback_panic", || {
        for _ in 0..4 {
            capture_once(false, true);
            capture_once(false, false);
            capture_once(true, false);
        }
    });
}

fn silent_output<T: cpal::SizedSample>(device: &cpal::Device, config: &cpal::StreamConfig) {
    let (tx, rx) = mpsc::channel();
    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |samples, _| {
                let mut apartment = APTTYPE::default();
                let mut qualifier = APTTYPEQUALIFIER::default();
                unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }
                    .expect("the WASAPI output worker must have COM initialized");
                samples.fill(T::EQUILIBRIUM);
                let _ = tx.send(());
            },
            |error| panic!("hardware output error: {error}"),
            None,
        )
        .expect("hardware output must initialize successfully");
    stream.play().unwrap();
    rx.recv_timeout(Duration::from_secs(5))
        .expect("output callback did not arrive");
    drop(stream);
}

#[test]
#[ignore = "requires a real Windows output device; no-device is a failure"]
fn hardware_output_repeated_start_stop() {
    isolated("hardware_output_repeated_start_stop", || {
        for _ in 0..8 {
            let device = cpal::default_host()
                .default_output_device()
                .expect("this explicitly requested hardware test requires an output device");
            let supported = device.default_output_config().unwrap();
            let config = supported.config();
            match supported.sample_format() {
                cpal::SampleFormat::F32 => silent_output::<f32>(&device, &config),
                cpal::SampleFormat::I16 => silent_output::<i16>(&device, &config),
                cpal::SampleFormat::U16 => silent_output::<u16>(&device, &config),
                other => panic!("hardware test needs a supported fixture format; got {other:?}"),
            }
        }
    });
}

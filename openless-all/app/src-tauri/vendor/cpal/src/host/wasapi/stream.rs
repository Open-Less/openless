use super::{com, device::Device, windows_err_to_cpal_err};
use crate::traits::StreamTrait;
use crate::{
    BackendSpecificError, BuildStreamError, Data, InputCallbackInfo, OutputCallbackInfo,
    PauseStreamError, PlayStreamError, SampleFormat, StreamConfig, StreamError,
};
use std::mem;
use std::ptr;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::Media::Audio;
use windows::Win32::System::SystemServices;
use windows::Win32::System::Threading;

/// Owns a kernel event, not a COM interface. The last owner closes the handle.
pub(super) struct OwnedEvent(Foundation::HANDLE);

// Windows event handles are process-wide synchronization objects, usable from
// any thread. No apartment-bound COM object is hidden in this wrapper.
unsafe impl Send for OwnedEvent {}
unsafe impl Sync for OwnedEvent {}

impl OwnedEvent {
    pub(super) fn new() -> Result<Self, windows::core::Error> {
        unsafe {
            Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null())).map(Self)
        }
    }

    pub(super) fn handle(&self) -> Foundation::HANDLE {
        self.0
    }
}

impl Drop for OwnedEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = Foundation::CloseHandle(self.0);
        }
    }
}

pub struct Stream {
    thread: Option<JoinHandle<()>>,
    commands: Sender<Command>,
    // Both the controller and worker retain this event until the worker exits.
    pending_scheduled_event: Arc<OwnedEvent>,
    // Preserve the original controller's !Send/!Sync contract even though the
    // shared kernel event itself is thread-safe.
    _not_send_sync: std::marker::PhantomData<*mut ()>,
}

// Constructed, used and destroyed exclusively within the audio worker.
// Intentionally !Send: none of its COM interfaces may cross into another thread.
struct RunContext {
    stream: StreamInner,
    handles: Vec<Foundation::HANDLE>,
    commands: Receiver<Command>,
}

pub enum Command {
    PlayStream,
    PauseStream,
    Terminate,
}

pub enum AudioClientFlow {
    Render {
        render_client: Audio::IAudioRenderClient,
    },
    Capture {
        capture_client: Audio::IAudioCaptureClient,
    },
}

pub struct StreamInner {
    // Field declaration order is destruction order: release all services before
    // the audio client, then close its event. All GetService/Release calls occur
    // in this same worker, including failed startup and callback unwinding.
    pub client_flow: AudioClientFlow,
    pub audio_clock: Audio::IAudioClock,
    pub audio_client: Audio::IAudioClient,
    pub playing: bool,
    pub max_frames_in_buffer: u32,
    pub bytes_per_frame: u16,
    pub config: crate::StreamConfig,
    pub sample_format: SampleFormat,
    #[cfg(debug_assertions)]
    pub owner_thread: thread::ThreadId,
    pub(super) event: OwnedEvent,
}

impl Stream {
    pub(crate) fn new_input<D, E>(
        device_id: Vec<u16>,
        config: StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Self::spawn(
            "cpal_wasapi_in",
            move || {
                let device = Device::from_id(&device_id)
                    .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;
                device.build_input_stream_raw_inner(&config, sample_format)
            },
            move |context| run_input(context, &mut data_callback, &mut error_callback),
        )
    }

    pub(crate) fn new_output<D, E>(
        device_id: Vec<u16>,
        config: StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Self::spawn(
            "cpal_wasapi_out",
            move || {
                let device = Device::from_id(&device_id)
                    .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;
                device.build_output_stream_raw_inner(&config, sample_format)
            },
            move |context| run_output(context, &mut data_callback, &mut error_callback),
        )
    }

    fn spawn(
        name: &str,
        build: impl FnOnce() -> Result<StreamInner, BuildStreamError> + Send + 'static,
        run: impl FnOnce(RunContext) + Send + 'static,
    ) -> Result<Stream, BuildStreamError> {
        let pending_scheduled_event =
            Arc::new(OwnedEvent::new().map_err(windows_err_to_cpal_err::<BuildStreamError>)?);
        let worker_event = Arc::clone(&pending_scheduled_event);
        let (commands, receiver) = channel();
        let (ready, startup) = sync_channel(1);
        let thread = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // No COM interface has crossed the thread boundary. The TLS guard
                // outlives every worker stack value, also during panic unwinding.
                let initialized = com::try_com_initialized()
                    .map_err(windows_err_to_cpal_err::<BuildStreamError>)
                    .and_then(|()| build());
                let stream = match initialized {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let context = RunContext {
                    handles: vec![worker_event.handle(), stream.event.handle()],
                    stream,
                    commands: receiver,
                };
                // Report success only once Activate/Initialize/GetService succeeded.
                // If the builder went away, dropping context releases everything here.
                if ready.send(Ok(())).is_ok() {
                    run(context);
                }
            })
            .map_err(|error| BackendSpecificError {
                description: format!("failed to spawn WASAPI stream worker: {error}"),
            })?;

        match startup.recv() {
            Ok(Ok(())) => Ok(Stream {
                thread: Some(thread),
                commands,
                pending_scheduled_event,
                _not_send_sync: std::marker::PhantomData,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(BackendSpecificError {
                    description: "WASAPI stream worker exited before reporting startup".into(),
                }
                .into())
            }
        }
    }

    fn push_command(&self, command: Command) -> Result<(), ()> {
        self.commands.send(command).map_err(|_| ())?;
        unsafe { Threading::SetEvent(self.pending_scheduled_event.handle()).map_err(|_| ()) }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.push_command(Command::Terminate);
        // A failed callback can already have disconnected commands. Still join;
        // never skip cleanup or panic again while another panic is unwinding.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // pending_scheduled_event closes after the worker can no longer wait on it.
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.push_command(Command::PlayStream)
            .map_err(|_| PlayStreamError::DeviceNotAvailable)
    }
    fn pause(&self) -> Result<(), PauseStreamError> {
        self.push_command(Command::PauseStream)
            .map_err(|_| PauseStreamError::DeviceNotAvailable)
    }
}

impl Drop for StreamInner {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        debug_assert_eq!(self.owner_thread, thread::current().id());
        if self.playing {
            unsafe {
                let _ = self.audio_client.Stop();
            }
            self.playing = false;
        }
        // Fields release services/client/event here, before worker TLS COM cleanup.
    }
}

// Process any pending commands that are queued within the `RunContext`.
// Returns `true` if the loop should continue running, `false` if it should terminate.
fn process_commands(run_context: &mut RunContext) -> Result<bool, StreamError> {
    // Process the pending commands.
    for command in run_context.commands.try_iter() {
        match command {
            Command::PlayStream => unsafe {
                if !run_context.stream.playing {
                    run_context
                        .stream
                        .audio_client
                        .Start()
                        .map_err(windows_err_to_cpal_err::<StreamError>)?;
                    run_context.stream.playing = true;
                }
            },
            Command::PauseStream => unsafe {
                if run_context.stream.playing {
                    run_context
                        .stream
                        .audio_client
                        .Stop()
                        .map_err(windows_err_to_cpal_err::<StreamError>)?;
                    run_context.stream.playing = false;
                }
            },
            Command::Terminate => {
                return Ok(false);
            }
        }
    }

    Ok(true)
}
// Wait for any of the given handles to be signalled.
//
// Returns the index of the `handle` that was signalled, or an `Err` if
// `WaitForMultipleObjectsEx` fails.
//
// This is called when the `run` thread is ready to wait for the next event. The
// next event might be some command submitted by the user (the first handle) or
// might indicate that one of the streams is ready to deliver or receive audio.
fn wait_for_handle_signal(handles: &[Foundation::HANDLE]) -> Result<usize, BackendSpecificError> {
    debug_assert!(handles.len() <= SystemServices::MAXIMUM_WAIT_OBJECTS as usize);
    let result = unsafe {
        Threading::WaitForMultipleObjectsEx(
            handles,
            false,               // Don't wait for all, just wait for the first
            Threading::INFINITE, // TODO: allow setting a timeout
            false,               // irrelevant parameter here
        )
    };
    if result == Foundation::WAIT_FAILED {
        let err = unsafe { Foundation::GetLastError() };
        let description = format!("`WaitForMultipleObjectsEx failed: {:?}", err);
        let err = BackendSpecificError { description };
        return Err(err);
    }
    // Notifying the corresponding task handler.
    let handle_idx = (result.0 - WAIT_OBJECT_0.0) as usize;
    Ok(handle_idx)
}

// Get the number of available frames that are available for writing/reading.
fn get_available_frames(stream: &StreamInner) -> Result<u32, StreamError> {
    unsafe {
        let padding = stream
            .audio_client
            .GetCurrentPadding()
            .map_err(windows_err_to_cpal_err::<StreamError>)?;
        Ok(stream.max_frames_in_buffer - padding)
    }
}

fn run_input(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&Data, &InputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) {
    boost_current_thread_priority();

    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            None => (),
        }
        let capture_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Capture { ref capture_client } => capture_client.clone(),
            _ => unreachable!(),
        };
        match process_input(
            &run_ctxt.stream,
            capture_client,
            data_callback,
            error_callback,
        ) {
            ControlFlow::Break => break,
            ControlFlow::Continue => continue,
        }
    }
}

fn run_output(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&mut Data, &OutputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) {
    boost_current_thread_priority();

    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            None => (),
        }
        let render_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Render { ref render_client } => render_client.clone(),
            _ => unreachable!(),
        };
        match process_output(
            &run_ctxt.stream,
            render_client,
            data_callback,
            error_callback,
        ) {
            ControlFlow::Break => break,
            ControlFlow::Continue => continue,
        }
    }
}

fn boost_current_thread_priority() {
    unsafe {
        let thread_id = Threading::GetCurrentThreadId();

        let _ = Threading::SetThreadPriority(
            HANDLE(thread_id as isize),
            Threading::THREAD_PRIORITY_TIME_CRITICAL,
        );
    }
}

enum ControlFlow {
    Break,
    Continue,
}

fn process_commands_and_await_signal(
    run_context: &mut RunContext,
    error_callback: &mut dyn FnMut(StreamError),
) -> Option<ControlFlow> {
    // Process queued commands.
    match process_commands(run_context) {
        Ok(true) => (),
        Ok(false) => return Some(ControlFlow::Break),
        Err(err) => {
            error_callback(err);
            return Some(ControlFlow::Break);
        }
    };

    // Wait for any of the handles to be signalled.
    let handle_idx = match wait_for_handle_signal(&run_context.handles) {
        Ok(idx) => idx,
        Err(err) => {
            error_callback(err.into());
            return Some(ControlFlow::Break);
        }
    };

    // If `handle_idx` is 0, then it's `pending_scheduled_event` that was signalled in
    // order for us to pick up the pending commands. Otherwise, a stream needs data.
    if handle_idx == 0 {
        return Some(ControlFlow::Continue);
    }

    None
}

// Each successfully acquired packet is released exactly once, including a
// timestamp error or a panic in user code. Aborted render packets are silent.
struct BufferRelease<F: FnOnce(bool) -> Result<(), windows::core::Error>> {
    release: Option<F>,
}

impl<F: FnOnce(bool) -> Result<(), windows::core::Error>> BufferRelease<F> {
    fn new(release: F) -> Self {
        Self {
            release: Some(release),
        }
    }

    fn finish(mut self) -> Result<(), windows::core::Error> {
        // Disarm before the COM call: an HRESULT failure must not cause a second
        // ReleaseBuffer call during destruction.
        self.release.take().unwrap()(false)
    }
}

impl<F: FnOnce(bool) -> Result<(), windows::core::Error>> Drop for BufferRelease<F> {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release(true);
        }
    }
}

// The loop for processing pending input data.
fn process_input(
    stream: &StreamInner,
    capture_client: Audio::IAudioCaptureClient,
    data_callback: &mut dyn FnMut(&Data, &InputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) -> ControlFlow {
    unsafe {
        // Get the available data in the shared buffer.
        let mut buffer: *mut u8 = ptr::null_mut();
        let mut flags = mem::MaybeUninit::uninit();
        loop {
            let mut frames_available = match capture_client.GetNextPacketSize() {
                Ok(0) => return ControlFlow::Continue,
                Ok(f) => f,
                Err(err) => {
                    error_callback(windows_err_to_cpal_err(err));
                    return ControlFlow::Break;
                }
            };
            let mut qpc_position: u64 = 0;
            let result = capture_client.GetBuffer(
                &mut buffer,
                &mut frames_available,
                flags.as_mut_ptr(),
                None,
                Some(&mut qpc_position),
            );

            match result {
                Err(e) => {
                    error_callback(windows_err_to_cpal_err(e));
                    return ControlFlow::Break;
                }
                Ok(_) => (),
            }

            // AUDCLNT_S_BUFFER_EMPTY is a success HRESULT, so windows-rs returns
            // Ok(()) and the frame count is zero. In this case GetBuffer leaves
            // the data pointer and timestamps untouched: do not turn that pointer
            // into a slice or invoke the callback. No packet needs to be released.
            if frames_available == 0 {
                return ControlFlow::Continue;
            }

            let packet = BufferRelease::new(|_| capture_client.ReleaseBuffer(frames_available));
            debug_assert!(!buffer.is_null());

            let data = buffer as *mut ();
            let len = frames_available as usize * stream.bytes_per_frame as usize
                / stream.sample_format.sample_size();
            let data = Data::from_parts(data, len, stream.sample_format);

            // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
            let timestamp = match input_timestamp(stream, qpc_position) {
                Ok(ts) => ts,
                Err(err) => {
                    error_callback(err);
                    return ControlFlow::Break;
                }
            };
            let info = InputCallbackInfo { timestamp };
            data_callback(&data, &info);

            // Release the buffer.
            let result = packet.finish().map_err(windows_err_to_cpal_err);
            if let Err(err) = result {
                error_callback(err);
                return ControlFlow::Break;
            }
        }
    }
}

// The loop for writing output data.
fn process_output(
    stream: &StreamInner,
    render_client: Audio::IAudioRenderClient,
    data_callback: &mut dyn FnMut(&mut Data, &OutputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) -> ControlFlow {
    // The number of frames available for writing.
    let frames_available = match get_available_frames(stream) {
        Ok(0) => return ControlFlow::Continue, // TODO: Can this happen?
        Ok(n) => n,
        Err(err) => {
            error_callback(err);
            return ControlFlow::Break;
        }
    };

    unsafe {
        let buffer = match render_client.GetBuffer(frames_available) {
            Ok(b) => b,
            Err(e) => {
                error_callback(windows_err_to_cpal_err(e));
                return ControlFlow::Break;
            }
        };

        let packet = BufferRelease::new(|aborted| {
            let flags = if aborted {
                Audio::AUDCLNT_BUFFERFLAGS_SILENT.0 as u32
            } else {
                0
            };
            render_client.ReleaseBuffer(frames_available, flags)
        });
        debug_assert!(!buffer.is_null());

        let data = buffer as *mut ();
        let len = frames_available as usize * stream.bytes_per_frame as usize
            / stream.sample_format.sample_size();
        let mut data = Data::from_parts(data, len, stream.sample_format);
        let sample_rate = stream.config.sample_rate;
        let timestamp = match output_timestamp(stream, frames_available, sample_rate) {
            Ok(ts) => ts,
            Err(err) => {
                error_callback(err);
                return ControlFlow::Break;
            }
        };
        let info = OutputCallbackInfo { timestamp };
        data_callback(&mut data, &info);

        if let Err(err) = packet.finish() {
            error_callback(windows_err_to_cpal_err(err));
            return ControlFlow::Break;
        }
    }

    ControlFlow::Continue
}

/// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: u32, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

/// Use the stream's `IAudioClock` to produce the current stream instant.
///
/// Uses the QPC position produced via the `GetPosition` method.
fn stream_instant(stream: &StreamInner) -> Result<crate::StreamInstant, StreamError> {
    let mut position: u64 = 0;
    let mut qpc_position: u64 = 0;
    unsafe {
        stream
            .audio_clock
            .GetPosition(&mut position, Some(&mut qpc_position))
            .map_err(windows_err_to_cpal_err::<StreamError>)?;
    };
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = qpc_position as i128 * 100;
    let instant = crate::StreamInstant::from_nanos_i128(qpc_nanos)
        .expect("performance counter out of range of `StreamInstant` representation");
    Ok(instant)
}

/// Produce the input stream timestamp.
///
/// `buffer_qpc_position` is the `qpc_position` returned via the `GetBuffer` call on the capture
/// client. It represents the instant at which the first sample of the retrieved buffer was
/// captured.
fn input_timestamp(
    stream: &StreamInner,
    buffer_qpc_position: u64,
) -> Result<crate::InputStreamTimestamp, StreamError> {
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = buffer_qpc_position as i128 * 100;
    let capture = crate::StreamInstant::from_nanos_i128(qpc_nanos)
        .expect("performance counter out of range of `StreamInstant` representation");
    let callback = stream_instant(stream)?;
    Ok(crate::InputStreamTimestamp { capture, callback })
}

/// Produce the output stream timestamp.
///
/// `frames_available` is the number of frames available for writing as reported by subtracting the
/// result of `GetCurrentPadding` from the maximum buffer size.
///
/// `sample_rate` is the rate at which audio frames are processed by the device.
///
/// TODO: The returned `playback` is an estimate that assumes audio is delivered immediately after
/// `frames_available` are consumed. The reality is that there is likely a tiny amount of latency
/// after this, but not sure how to determine this.
fn output_timestamp(
    stream: &StreamInner,
    frames_available: u32,
    sample_rate: crate::SampleRate,
) -> Result<crate::OutputStreamTimestamp, StreamError> {
    let callback = stream_instant(stream)?;
    let buffer_duration = frames_to_duration(frames_available, sample_rate);
    let playback = callback
        .add(buffer_duration)
        .expect("`playback` occurs beyond representation supported by `StreamInstant`");
    Ok(crate::OutputStreamTimestamp { callback, playback })
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use windows::Win32::System::Com::{CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER};

    #[test]
    fn startup_failure_is_returned_after_worker_cleanup() {
        let caller = thread::current().id();
        let (tx, rx) = channel();
        struct BeforeComCleanup(Sender<thread::ThreadId>);
        impl Drop for BeforeComCleanup {
            fn drop(&mut self) {
                let mut apartment = APTTYPE::default();
                let mut qualifier = APTTYPEQUALIFIER::default();
                unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }
                    .expect("worker values must drop before COM is uninitialized");
                self.0.send(thread::current().id()).unwrap();
            }
        }
        let result = Stream::spawn(
            "cpal_startup_failure_test",
            move || {
                let _cleanup = BeforeComCleanup(tx);
                Err(BuildStreamError::StreamConfigNotSupported)
            },
            |_| panic!("a failed stream must not enter its callback loop"),
        );
        assert!(matches!(
            result,
            Err(BuildStreamError::StreamConfigNotSupported)
        ));
        assert_ne!(rx.recv().unwrap(), caller);
    }

    #[test]
    fn acquired_packet_releases_once_on_success_error_and_unwind() {
        use std::cell::RefCell;
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let releases = RefCell::new(Vec::new());
        BufferRelease::new(|aborted| {
            releases.borrow_mut().push(aborted);
            Ok(())
        })
        .finish()
        .unwrap();
        let failed = BufferRelease::new(|aborted| {
            releases.borrow_mut().push(aborted);
            Err(windows::core::Error::from_hresult(Foundation::E_FAIL))
        })
        .finish();
        assert!(failed.is_err());
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _packet = BufferRelease::new(|aborted| {
                releases.borrow_mut().push(aborted);
                Ok(())
            });
            panic!("callback failure while a packet is acquired");
        }));
        assert!(panic.is_err());
        assert_eq!(*releases.borrow(), [false, false, true]);
    }

    #[test]
    fn missing_endpoint_is_reported_synchronously() {
        let id: Vec<u16> = "openless-invalid-endpoint-id"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let result = Stream::new_input(
            id,
            StreamConfig {
                channels: 1,
                sample_rate: crate::SampleRate(16_000),
                buffer_size: crate::BufferSize::Default,
            },
            SampleFormat::F32,
            |_, _| panic!("an invalid endpoint must never invoke a data callback"),
            |_| panic!("startup errors must be returned to the builder"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn startup_panic_returns_an_error_instead_of_panicking_in_drop() {
        let result = Stream::spawn(
            "cpal_startup_panic_test",
            || panic!("intentional startup panic"),
            |_| panic!("startup never completed"),
        );
        assert!(matches!(
            result,
            Err(BuildStreamError::BackendSpecific { .. })
        ));
    }
}

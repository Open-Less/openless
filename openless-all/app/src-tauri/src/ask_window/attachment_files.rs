use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use openless_core::ask_attachment::{
    decode_image, pdf, AttachmentContent, AttachmentError, AttachmentMetadata, AttachmentScope,
    FileKind, PendingAttachment, MAX_IMAGE_BYTES,
};
use tauri_plugin_dialog::DialogExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{oneshot, watch, Mutex},
};

const PDF_WORKER_FLAG: &str = "--openless-ask-pdf-worker";
const PDF_WORKER_MEMORY: usize = 512 * 1024 * 1024;
const PDF_WORKER_CPU_SECONDS: u64 = 5;
const PDF_WORKER_TIMEOUT: Duration = Duration::from_secs(8);
static PICKER: Mutex<()> = Mutex::const_new(());

/// Only the native picker can create a source that reads a path.
pub(crate) struct NativeAttachmentFile {
    display_name: String,
    kind: FileKind,
    source: AttachmentFileSource,
}

enum AttachmentFileSource {
    Picker(PathBuf),
    DropBytes(Vec<u8>),
}

pub(super) fn decode_dropped_image(
    display_name: &str,
    data_base64: &str,
) -> Result<NativeAttachmentFile, AttachmentError> {
    if data_base64.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
        return Err(AttachmentError::ImageTooLarge);
    }
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| AttachmentError::ImageInvalid)?;
    NativeAttachmentFile::from_image_drop(display_name, bytes)
}

pub(crate) async fn pick_attachment_file(
    window: &tauri::WebviewWindow,
) -> Result<Option<NativeAttachmentFile>, AttachmentError> {
    if window.label() != super::LABEL {
        return Err(AttachmentError::PickerUnavailable);
    }
    let guard = PICKER.try_lock().map_err(|_| AttachmentError::PickerBusy)?;
    let (sender, receiver) = oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(window)
        .set_title("Attach to Ask")
        .add_filter("PNG, JPEG, or PDF", &["png", "jpg", "jpeg", "pdf"])
        .pick_file(move |file| {
            drop(guard);
            let _ = sender.send(file);
        });
    let Some(file) = receiver
        .await
        .map_err(|_| AttachmentError::PickerUnavailable)?
    else {
        return Ok(None);
    };
    let path = file
        .as_path()
        .ok_or(AttachmentError::UnsupportedFile)?
        .to_path_buf();
    NativeAttachmentFile::new(path, true).map(Some)
}

impl NativeAttachmentFile {
    /// Takes already-decoded IPC bytes, never a path to open.
    pub(crate) fn from_image_drop(name: &str, bytes: Vec<u8>) -> Result<Self, AttachmentError> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(AttachmentError::ImageTooLarge);
        }
        let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
        let basename = match name.as_bytes() {
            [drive, b':', ..] if drive.is_ascii_alphabetic() => &name[2..],
            _ => name,
        };
        if basename.len() > 1024 || basename.chars().any(char::is_control) {
            return Err(AttachmentError::UnsupportedFile);
        }
        let kind = file_kind(Path::new(basename), false)?;
        Ok(Self {
            kind,
            display_name: basename.to_owned(),
            source: AttachmentFileSource::DropBytes(bytes),
        })
    }

    fn new(path: PathBuf, allow_pdf: bool) -> Result<Self, AttachmentError> {
        let kind = file_kind(&path, allow_pdf)?;
        let display_name = path
            .file_name()
            .ok_or(AttachmentError::UnsupportedFile)?
            .to_string_lossy()
            .into_owned();
        Ok(Self {
            kind,
            display_name,
            source: AttachmentFileSource::Picker(path),
        })
    }

    /// Register under the host's conversation/event lock and publish the processing metadata first.
    pub(crate) fn register(
        self,
        scope: &AttachmentScope,
    ) -> Result<RegisteredAttachmentFile, AttachmentError> {
        let pending = scope.begin_file(self.display_name, self.kind)?;
        Ok(RegisteredAttachmentFile {
            pending,
            kind: self.kind,
            source: self.source,
        })
    }
}

fn file_kind(name: &Path, allow_pdf: bool) -> Result<FileKind, AttachmentError> {
    let extension = name.extension().and_then(OsStr::to_str);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("png") => Ok(FileKind::Png),
        Some("jpg" | "jpeg") => Ok(FileKind::Jpeg),
        Some("pdf") if allow_pdf => Ok(FileKind::Pdf),
        _ => Err(AttachmentError::UnsupportedFile),
    }
}

pub(crate) struct RegisteredAttachmentFile {
    pending: PendingAttachment,
    kind: FileKind,
    source: AttachmentFileSource,
}

impl RegisteredAttachmentFile {
    pub(crate) fn metadata(&self) -> &AttachmentMetadata {
        self.pending.metadata()
    }

    pub(crate) async fn process(self) -> FinishedAttachmentFile {
        let Self {
            pending,
            kind,
            source,
        } = self;
        let result = process_file(&pending, kind, source).await;
        FinishedAttachmentFile { pending, result }
    }
}

pub(crate) struct FinishedAttachmentFile {
    pending: PendingAttachment,
    result: Result<AttachmentContent, AttachmentError>,
}

impl FinishedAttachmentFile {
    /// Commit under the same lock as add/remove/end; publish only the metadata returned here.
    pub(crate) fn commit(self) -> Option<AttachmentMetadata> {
        self.pending.finish(self.result)
    }
}

enum ReadAttachmentFile {
    Pdf(Vec<u8>),
    Image(openless_core::ask_attachment::VerifiedImage),
}

async fn process_file(
    pending: &PendingAttachment,
    kind: FileKind,
    source: AttachmentFileSource,
) -> Result<AttachmentContent, AttachmentError> {
    let permit = Arc::new(pending.processing_slot().await?);
    let blocking_permit = permit.clone();
    let cancellation = pending.cancellation();
    let file = tokio::task::spawn_blocking(move || {
        let _permit = blocking_permit;
        let bytes = match source {
            AttachmentFileSource::Picker(path) => read_selected_file(&path, kind, &cancellation)?,
            AttachmentFileSource::DropBytes(bytes) => bytes,
        };
        check_cancelled(&cancellation)?;
        match kind {
            FileKind::Pdf => Ok(ReadAttachmentFile::Pdf(bytes)),
            FileKind::Png | FileKind::Jpeg => {
                decode_image(kind, bytes).map(ReadAttachmentFile::Image)
            }
        }
    })
    .await
    .map_err(|_| AttachmentError::FileUnreadable)??;
    check_cancelled(&pending.cancellation())?;
    match file {
        ReadAttachmentFile::Image(image) => Ok(AttachmentContent::Image(image)),
        ReadAttachmentFile::Pdf(bytes) => parse_pdf(bytes, pending.cancellation())
            .await
            .map(AttachmentContent::Pdf),
    }
}

fn read_selected_file(
    path: &Path,
    kind: FileKind,
    cancellation: &watch::Receiver<()>,
) -> Result<Vec<u8>, AttachmentError> {
    check_cancelled(cancellation)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| AttachmentError::FileUnreadable)?;
    let metadata = file
        .metadata()
        .map_err(|_| AttachmentError::FileUnreadable)?;
    if !metadata.is_file() {
        return Err(AttachmentError::FileUnreadable);
    }
    let max_bytes = kind.max_bytes();
    let too_large = match kind {
        FileKind::Pdf => AttachmentError::PdfTooLarge,
        FileKind::Png | FileKind::Jpeg => AttachmentError::ImageTooLarge,
    };
    if metadata.len() > max_bytes as u64 {
        return Err(too_large);
    }
    let mut file = file.take((max_bytes + 1) as u64);
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        check_cancelled(cancellation)?;
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) if count <= max_bytes - bytes.len() => {
                bytes.extend_from_slice(&chunk[..count])
            }
            Ok(_) => return Err(too_large),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(AttachmentError::FileUnreadable),
        }
    }
    Ok(bytes)
}

fn check_cancelled(cancellation: &watch::Receiver<()>) -> Result<(), AttachmentError> {
    cancellation
        .has_changed()
        .map(|_| ())
        .map_err(|_| AttachmentError::Cancelled)
}

async fn parse_pdf(
    bytes: Vec<u8>,
    mut cancellation: watch::Receiver<()>,
) -> Result<pdf::PdfText, AttachmentError> {
    check_cancelled(&cancellation)?;
    let executable = std::env::current_exe().map_err(|_| AttachmentError::PdfUnavailable)?;
    let mut command = Command::new(executable);
    command
        .arg(PDF_WORKER_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "macos")]
    command.env("MallocNanoZone", "0");
    #[cfg(windows)]
    command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    let mut child = command
        .spawn()
        .map_err(|_| AttachmentError::PdfUnavailable)?;
    let mut input = child.stdin.take().ok_or(AttachmentError::PdfUnavailable)?;
    let output = child.stdout.take().ok_or(AttachmentError::PdfUnavailable)?;

    let exchange = async {
        let write = async move {
            match input.write_all(&bytes).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                Err(_) => Err(AttachmentError::PdfUnavailable),
            }
        };
        let read = async move {
            let mut buffer = Vec::new();
            output
                .take((pdf::MAX_WORKER_REPLY_BYTES + 1) as u64)
                .read_to_end(&mut buffer)
                .await
                .map_err(|_| AttachmentError::PdfUnavailable)?;
            if buffer.len() > pdf::MAX_WORKER_REPLY_BYTES {
                return Err(AttachmentError::PdfTooLarge);
            }
            Ok(buffer)
        };
        let ((), reply) = tokio::try_join!(write, read)?;
        let status = child
            .wait()
            .await
            .map_err(|_| AttachmentError::PdfUnavailable)?;
        if !status.success() {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                match status.signal() {
                    Some(libc::SIGXCPU | libc::SIGKILL) => {
                        return Err(AttachmentError::PdfTimedOut)
                    }
                    Some(_) => return Err(AttachmentError::PdfInvalid),
                    None => {}
                }
            }
            return Err(AttachmentError::PdfUnavailable);
        }
        pdf::read_worker_reply(&reply)
    };
    let result = tokio::select! {
        biased;
        _ = cancellation.changed() => Err(AttachmentError::Cancelled),
        result = tokio::time::timeout(PDF_WORKER_TIMEOUT, exchange) => {
            result.unwrap_or(Err(AttachmentError::PdfTimedOut))
        }
    };
    if result.is_err() && matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill().await;
    }
    result
}

/// Call before Tauri, single-instance handling, logging, or any other worker startup.
pub(crate) fn run_pdf_worker_if_requested() -> Option<i32> {
    if std::env::args_os().nth(1).as_deref() != Some(OsStr::new(PDF_WORKER_FLAG)) {
        return None;
    }
    let limits = install_pdf_worker_limits().map_err(|_| AttachmentError::PdfUnavailable);
    let mut stdout = io::stdout().lock();
    let result = pdf::write_worker_reply(limits, io::stdin().lock(), &mut stdout)
        .and_then(|()| std::io::Write::flush(&mut stdout));
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(unix)]
fn install_pdf_worker_limits() -> io::Result<()> {
    fn set(resource: libc::c_int, soft: libc::rlim_t, hard: libc::rlim_t) -> io::Result<()> {
        let mut inherited = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(resource as _, &mut inherited) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let limit = libc::rlimit {
            rlim_cur: soft.min(inherited.rlim_cur),
            rlim_max: hard.min(inherited.rlim_max),
        };
        if unsafe { libc::setrlimit(resource as _, &limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    set(libc::RLIMIT_CORE as _, 0, 0)?;
    let memory_limit = pdf_worker_address_limit()?;
    set(libc::RLIMIT_AS as _, memory_limit, memory_limit)?;
    set(
        libc::RLIMIT_CPU as _,
        PDF_WORKER_CPU_SECONDS as _,
        (PDF_WORKER_CPU_SECONDS + 1) as _,
    )
}

#[cfg(target_os = "macos")]
fn pdf_worker_address_limit() -> io::Result<libc::rlim_t> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let count = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count != size {
        return Err(io::Error::last_os_error());
    }
    let baseline = unsafe { info.assume_init() }.pti_virtual_size;
    baseline
        .checked_add(PDF_WORKER_MEMORY as u64)
        .ok_or_else(|| io::Error::other("PDF worker address space limit overflow"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn pdf_worker_address_limit() -> io::Result<libc::rlim_t> {
    Ok(PDF_WORKER_MEMORY as libc::rlim_t)
}

#[cfg(windows)]
fn install_pdf_worker_limits() -> io::Result<()> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
                    JOB_OBJECT_LIMIT_PROCESS_TIME,
                },
                Threading::GetCurrentProcess,
            },
        },
    };

    let result = unsafe {
        CreateJobObjectW(None, PCWSTR::null()).and_then(|job| {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_TIME
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            limits.ProcessMemoryLimit = PDF_WORKER_MEMORY;
            limits.BasicLimitInformation.ActiveProcessLimit = 1;
            limits.BasicLimitInformation.PerProcessUserTimeLimit =
                PDF_WORKER_CPU_SECONDS as i64 * 10_000_000;
            let result = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
            .and_then(|()| AssignProcessToJobObject(job, GetCurrentProcess()));
            let _ = CloseHandle(job);
            result
        })
    };
    result.map_err(io::Error::other)
}

#[cfg(not(any(unix, windows)))]
fn install_pdf_worker_limits() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "PDF worker limits are unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use openless_core::ask_attachment::{AttachmentReader, AttachmentRegistry, AttachmentState};

    use super::*;

    #[tokio::test]
    async fn image_drop_bytes_share_validation_without_opening_paths() {
        let png = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGPgUbIAAACkAGeY0OCYAAAAAElFTkSuQmCC")
            .unwrap();
        let registry = AttachmentRegistry::default();
        let scope = registry.open_conversation("active");
        let dropped =
            NativeAttachmentFile::from_image_drop(r"C:\fakepath\photo.PNG", png.clone()).unwrap();
        let job = dropped.register(&scope).unwrap();
        assert_eq!(job.metadata().display_name, "photo.PNG");
        let ready = job.process().await.commit().unwrap();
        assert_eq!(ready.state, AttachmentState::Ready);

        let lease = registry
            .lease("active", "turn", &[ready.attachment_id])
            .unwrap();
        let AttachmentContent::Image(image) = lease.attachments()[0].content() else {
            panic!("expected image");
        };
        assert_eq!(image.bytes(), png);

        let invalid =
            NativeAttachmentFile::from_image_drop("/not/a/real/path/photo.jpg", png.clone())
                .unwrap()
                .register(&scope)
                .unwrap();
        assert_eq!(invalid.metadata().display_name, "photo.jpg");
        let invalid = invalid.process().await.commit().unwrap();
        assert_eq!(invalid.state, AttachmentState::Error);
        assert_eq!(invalid.error.unwrap().code, AttachmentError::ImageInvalid);
        assert_eq!(
            NativeAttachmentFile::from_image_drop("document.pdf", png).err(),
            Some(AttachmentError::UnsupportedFile),
        );
        assert_eq!(
            NativeAttachmentFile::from_image_drop("photo.png", vec![0; MAX_IMAGE_BYTES + 1]).err(),
            Some(AttachmentError::ImageTooLarge),
        );
    }
}

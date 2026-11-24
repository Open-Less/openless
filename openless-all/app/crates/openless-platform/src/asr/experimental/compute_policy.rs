//! macOS inference resource policy, independent of capture/UI threads.
//! Budgets are CPU-seconds per wall-second (1.0 = one logical core), not
//! Activity Monitor's per-core percentage. Native calls can briefly burst;
//! cooperative checkpoints bound average CPU, not instantaneous utilization.
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

const IDLE_SHARE: f64 = 0.8;
const MAX_CPU_THREADS: i32 = 2;

#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    pub(super) cores: f64,
    pub(super) threads: i32,
}
impl Budget {
    fn from_idle(logical_cores: usize, idle_fraction: f64) -> Self {
        let idle = if idle_fraction.is_finite() {
            idle_fraction.clamp(0.0, 1.0)
        } else {
            0.25 // Missing telemetry must never mean unlimited CPU.
        };
        // Native worker pools (including Core ML/MLX helpers) can exceed the
        // requested BLAS/ONNX thread count. Bound their aggregate average too.
        let cores =
            (logical_cores.max(1) as f64 * idle * IDLE_SHARE).clamp(0.05, MAX_CPU_THREADS as f64);
        Self {
            cores,
            threads: (cores.floor() as i32).clamp(1, MAX_CPU_THREADS),
        }
    }
    /// Call only on a blocking inference worker, never an audio/UI callback.
    pub(super) fn capture() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let budget = Self::from_idle(cores, idle_fraction().unwrap_or(0.25));
        log::info!(
            "[experimental-asr] CPU budget={:.2} core(s), native threads={}, idle share=80%, aggregate cap=2 cores",
            budget.cores,
            budget.threads
        );
        budget
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn cpu_ticks() -> Option<[u32; 4]> {
    let mut info = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: this flavor writes exactly host_cpu_load_info; count describes
    // its integer storage. Balance the acquired host send right on all paths.
    let result = unsafe {
        let host = libc::mach_host_self();
        let result = libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            (&mut info as *mut libc::host_cpu_load_info).cast(),
            &mut count,
        );
        mach_port_deallocate(libc::mach_task_self(), host);
        result
    };
    (result == libc::KERN_SUCCESS && count == libc::HOST_CPU_LOAD_INFO_COUNT)
        .then_some(info.cpu_ticks)
}

#[cfg(target_os = "macos")]
fn idle_fraction() -> Option<f64> {
    let before = cpu_ticks()?;
    std::thread::sleep(Duration::from_millis(100));
    let after = cpu_ticks()?;
    let delta = std::array::from_fn::<_, 4, _>(|i| after[i].wrapping_sub(before[i]) as u64);
    let total: u64 = delta.iter().sum();
    (total != 0).then(|| delta[libc::CPU_STATE_IDLE as usize] as f64 / total as f64)
}
#[cfg(not(target_os = "macos"))]
fn idle_fraction() -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
fn cpu_seconds() -> Option<f64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: time points to initialized, writable timespec storage.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) };
    (result == 0).then_some(time.tv_sec as f64 + time.tv_nsec as f64 / 1e9)
}
#[cfg(not(target_os = "macos"))]
fn cpu_seconds() -> Option<f64> {
    None
}

fn delay(cpu_used: f64, wall_used: f64, cores: f64) -> f64 {
    (cpu_used / cores - wall_used).max(0.0)
}

/// Accounts for native worker pools, not just the calling thread. Including
/// other OpenLess CPU work is deliberately conservative: interaction makes
/// inference yield more, never slows microphone/UI execution.
pub(super) struct Pacer {
    budget: Budget,
    wall: Cell<Instant>,
    cpu: Cell<Option<f64>>,
}
impl Pacer {
    pub(super) fn new(budget: Budget) -> Self {
        Self {
            budget,
            wall: Cell::new(Instant::now()),
            cpu: Cell::new(cpu_seconds()),
        }
    }
    pub(super) fn checkpoint(&self, cancelled: impl Fn() -> bool) -> bool {
        if cancelled() {
            return false;
        }
        if let Some((start, now)) = self.cpu.get().zip(cpu_seconds()) {
            let wait = delay(
                (now - start).max(0.0),
                self.wall.get().elapsed().as_secs_f64(),
                self.budget.cores,
            );
            let resume = Instant::now() + Duration::from_secs_f64(wait);
            while Instant::now() < resume {
                if cancelled() {
                    return false;
                }
                std::thread::sleep(
                    resume
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(50)),
                );
            }
        }
        // Do not charge UI/recording CPU while inference is asleep. Continually
        // re-reading process CPU during the wait can otherwise create infinite
        // debt on a busy app, despite the inference thread already yielding.
        self.cpu.set(cpu_seconds());
        self.wall.set(Instant::now());
        !cancelled()
    }
}

// QoS applies only to the blocking inference thread and any workers it creates.
// Never lower the entire app/process (that would also penalize recording/UI).
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn pthread_get_qos_class_np(
        thread: libc::pthread_t,
        class: *mut u32,
        priority: *mut i32,
    ) -> i32;
    fn pthread_set_qos_class_self_np(class: u32, priority: i32) -> i32;
}
pub(super) struct BackgroundScope {
    #[cfg(target_os = "macos")]
    previous: Option<(u32, i32)>,
}
impl BackgroundScope {
    pub(super) fn enter() -> Self {
        #[cfg(target_os = "macos")]
        {
            let (mut class, mut priority) = (0, 0);
            // SAFETY: query and change only the current pthread; output storage
            // is valid and utility is a public Darwin QoS class (0x11).
            let previous = unsafe {
                (pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) == 0
                    && pthread_set_qos_class_self_np(0x11, 0) == 0)
                    .then_some((class, priority))
            };
            Self { previous }
        }
        #[cfg(not(target_os = "macos"))]
        Self {}
    }
}
impl Drop for BackgroundScope {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some((class, priority)) = self.previous {
            // Unspecified cannot be requested with the setter; restore default
            // instead of leaking utility QoS into Tokio's reusable worker.
            let class = if class == 0 { 0x15 } else { class };
            // SAFETY: restore this same thread's captured QoS before returning
            // the blocking worker to the runtime.
            unsafe {
                pthread_set_qos_class_self_np(class, priority);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_eighty_percent_of_idle_capacity_with_two_core_aggregate_ceiling() {
        let budget = Budget::from_idle(10, 0.9);
        assert_eq!(budget.cores, 2.0);
        assert_eq!(budget.threads, 2);
        let busy = Budget::from_idle(10, 0.1);
        assert!((busy.cores - 0.8).abs() < 1e-9);
        assert_eq!(busy.threads, 1);
    }
    #[test]
    fn invalid_telemetry_and_tiny_machines_remain_bounded() {
        for idle in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1.0, 8.0] {
            let budget = Budget::from_idle(1, idle);
            assert!(budget.cores.is_finite() && budget.cores > 0.0 && budget.cores <= 0.8);
            assert_eq!(budget.threads, 1);
        }
    }
    #[test]
    fn pacing_accounts_for_all_native_threads_and_gpu_wait_time() {
        assert!((delay(2.0, 1.0, 0.5) - 3.0).abs() < 1e-9);
        assert_eq!(delay(0.2, 2.0, 0.5), 0.0, "GPU/wait time is not CPU work");
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cpu_telemetry_and_thread_scope_are_available() {
        let before = cpu_seconds().expect("process CPU clock");
        let idle = idle_fraction().expect("host CPU counters");
        assert!((0.0..=1.0).contains(&idle));
        assert!(cpu_seconds().unwrap() >= before);
        let mut previous = (0u32, 0i32);
        // SAFETY: output storage belongs to this test's current thread.
        assert_eq!(
            unsafe {
                pthread_get_qos_class_np(libc::pthread_self(), &mut previous.0, &mut previous.1)
            },
            0
        );
        {
            let _scope = BackgroundScope::enter();
            let mut class = 0;
            // SAFETY: querying this same live thread, with valid output storage.
            assert_eq!(
                unsafe {
                    pthread_get_qos_class_np(libc::pthread_self(), &mut class, std::ptr::null_mut())
                },
                0
            );
            assert_eq!(class, 0x11);
        }
        let mut restored = 0;
        // SAFETY: as above.
        unsafe {
            pthread_get_qos_class_np(libc::pthread_self(), &mut restored, std::ptr::null_mut());
        }
        assert_eq!(restored, if previous.0 == 0 { 0x15 } else { previous.0 });
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn real_cpu_work_obeys_the_cooperative_average_budget() {
        let _background = BackgroundScope::enter();
        let budget = Budget {
            cores: 0.25,
            threads: 1,
        };
        let pacer = Pacer::new(budget);
        let cpu = cpu_seconds().unwrap();
        let wall = Instant::now();
        while cpu_seconds().unwrap() - cpu < 0.05 {
            for i in 0..1000 {
                std::hint::black_box((i as f64).sqrt());
            }
        }
        // The budget charges CPU work before yielding, not other app/test
        // threads running while inference is asleep. Freeze the same boundary
        // here; measuring process CPU after the yield races parallel tests.
        let used = cpu_seconds().unwrap() - cpu;
        assert!(pacer.checkpoint(|| false));
        assert!(
            used / wall.elapsed().as_secs_f64() < 0.28,
            "measured CPU work must include the enforced yield"
        );
    }

    #[test]
    fn cancellation_interrupts_pacing_without_sleeping() {
        let pacer = Pacer::new(Budget::from_idle(1, 0.1));
        assert!(!pacer.checkpoint(|| true));
    }
}

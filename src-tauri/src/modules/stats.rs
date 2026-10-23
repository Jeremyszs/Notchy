//! System stats for the Stats page: CPU, memory, network and the system drive. A sampler reads them
//! once a second, and only while the page is on screen. The frontend says so with `stats_set_active`,
//! so an idle notch reads nothing. Each reading goes to `stats://update`.
//!
//! The arithmetic (CPU and network deltas, the percentages, smoothing) is pure, so the rules are
//! tested without Win32. The reads are plain calls that need no admin, COM or driver. Nothing is
//! stored or sent off the machine.

use std::iter;
use std::mem;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::PCWSTR;

use super::{Module, ModuleContext};
use crate::config;

/// One reading. Mirrors `StatsSnapshot` in src/lib/stats.ts.
const STATS_EVENT: &str = "stats://update";
/// How often the counters are read while the page is shown.
const INTERVAL: Duration = Duration::from_secs(1);
/// How much each new reading counts in the smoothed CPU and network rates, from 0 to 1. Lower is
/// steadier and slower to follow a change.
const SMOOTHING: f64 = 0.5;

/// Cumulative CPU time in 100 ns units. `total` is kernel plus user time, and kernel time already
/// includes idle, so busy time is `total - idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Bytes received and sent, summed over the interfaces that are up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetBytes {
    pub received: u64,
    pub sent: u64,
}

/// One read of the counters. A counter that fails to read is None, and only its rate is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub cpu: Option<CpuTimes>,
    pub net: Option<NetBytes>,
}

/// The rates derived from two reads, in percent and bytes per second. None until there are two
/// reads to compare, and when a counter went back, such as an adapter leaving.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rates {
    pub cpu: Option<f64>,
    pub down: Option<f64>,
    pub up: Option<f64>,
}

/// The whole reading. Memory and disk are the current values; the rates come from `Tracker`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub cpu: Option<f64>,
    pub memory: Memory,
    pub network: Network,
    pub disk: Disk,
}

/// Bytes in use and in total, and the share in use (0 to 100).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub percent: f64,
}

/// Bytes per second. None until the second reading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub down: Option<f64>,
    pub up: Option<f64>,
}

/// The system drive: bytes in total and free for this user, and the share in use (0 to 100).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub total: u64,
    pub free: u64,
    pub percent: f64,
}

impl Snapshot {
    /// Builds a reading from the rates and the two `(total, free)` pairs.
    pub fn new(rates: Rates, memory: (u64, u64), disk: (u64, u64)) -> Self {
        let (mem_total, mem_free) = memory;
        let (disk_total, disk_free) = disk;
        Self {
            cpu: rates.cpu,
            memory: Memory {
                used: mem_total.saturating_sub(mem_free),
                total: mem_total,
                percent: used_percent(mem_total, mem_free),
            },
            network: Network {
                down: rates.down,
                up: rates.up,
            },
            disk: Disk {
                total: disk_total,
                free: disk_free,
                percent: used_percent(disk_total, disk_free),
            },
        }
    }
}

/// The busy share of the CPU between two reads, from 0 to 100. None when no time passed or a counter
/// went back.
pub fn cpu_percent(before: CpuTimes, now: CpuTimes) -> Option<f64> {
    let total = now.total.checked_sub(before.total)?;
    let idle = now.idle.checked_sub(before.idle)?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some((busy as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
}

/// Bytes per second between two reads. None when no time passed, or the counter went back (a reset,
/// or an interface that left the total), since the difference would not mean traffic.
pub fn rate(before: u64, now: u64, elapsed: Duration) -> Option<f64> {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 || now < before {
        return None;
    }
    Some((now - before) as f64 / seconds)
}

/// The share of `total` that is not `free`, from 0 to 100. Zero when the total is unknown.
pub fn used_percent(total: u64, free: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let used = total.saturating_sub(free);
    (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
}

/// Blends a raw reading into the smoothed value. A missing reading stays missing, and the next real
/// one starts the smoothing again.
fn smooth(previous: Option<f64>, raw: Option<f64>) -> Option<f64> {
    let value = raw?;
    Some(match previous {
        Some(before) => before + (value - before) * SMOOTHING,
        None => value,
    })
}

/// Turns successive reads into smoothed rates. The first read of a run has nothing to compare with,
/// so it gives no rates. Pure: the caller passes the time of each read.
#[derive(Debug, Default)]
pub struct Tracker {
    last: Option<(Totals, Instant)>,
    smoothed: Rates,
}

impl Tracker {
    pub fn observe(&mut self, totals: Totals, now: Instant) -> Rates {
        let Some((before, at)) = self.last.replace((totals, now)) else {
            return Rates::default();
        };
        let elapsed = now.saturating_duration_since(at);
        let raw = Rates {
            cpu: before
                .cpu
                .zip(totals.cpu)
                .and_then(|(old, new)| cpu_percent(old, new)),
            down: before
                .net
                .zip(totals.net)
                .and_then(|(old, new)| rate(old.received, new.received, elapsed)),
            up: before
                .net
                .zip(totals.net)
                .and_then(|(old, new)| rate(old.sent, new.sent, elapsed)),
        };
        self.smoothed = Rates {
            cpu: smooth(self.smoothed.cpu, raw.cpu),
            down: smooth(self.smoothed.down, raw.down),
            up: smooth(self.smoothed.up, raw.up),
        };
        self.smoothed
    }

    /// Starts over. The next read gives no rates, as the first read of a run does.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct StatsModule;

impl Module for StatsModule {
    fn name(&self) -> &'static str {
        "stats"
    }

    fn start(&self, ctx: ModuleContext) {
        let (requests, inbox) = mpsc::channel();
        ctx.app.manage(Requests(Mutex::new(requests)));
        let app = ctx.app.clone();
        thread::spawn(move || run_sampler(app, inbox));
    }
}

/// The frontend's latest word on whether the page is shown.
pub struct Requests(Mutex<Sender<bool>>);

/// Sleeps until the page asks for samples. While it is shown, reads once per `INTERVAL`. The first
/// reading after the page opens is sent at once, with no rates, so the tiles fill in without a wait.
fn run_sampler(app: AppHandle, inbox: Receiver<bool>) {
    let mut tracker = Tracker::default();
    let mut active = false;
    loop {
        let next = if active {
            inbox.recv_timeout(INTERVAL)
        } else {
            inbox.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(on) => {
                let on = on && config::stats_enabled(&app);
                if on != active {
                    active = on;
                    tracker.reset();
                    if on {
                        read_and_emit(&app, &mut tracker);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => read_and_emit(&app, &mut tracker),
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn read_and_emit(app: &AppHandle, tracker: &mut Tracker) {
    if let Some(snapshot) = sample(tracker, Instant::now()) {
        let _ = app.emit(STATS_EVENT, snapshot);
    }
}

/// One full reading. None when memory or the drive cannot be read, so nothing is sent for that tick.
fn sample(tracker: &mut Tracker, now: Instant) -> Option<Snapshot> {
    let memory = memory()?;
    let disk = disk()?;
    let rates = tracker.observe(
        Totals {
            cpu: cpu_times(),
            net: net_bytes(),
        },
        now,
    );
    Some(Snapshot::new(rates, memory, disk))
}

/// Maps a FILETIME to 100 ns units.
fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn cpu_times() -> Option<CpuTimes> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some(CpuTimes {
        idle: ticks(idle),
        total: ticks(kernel).saturating_add(ticks(user)),
    })
}

/// `(total, available)` physical memory in bytes.
fn memory() -> Option<(u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some((status.ullTotalPhys, status.ullAvailPhys))
}

/// `(total, free for this user)` bytes on the system drive.
fn disk() -> Option<(u64, u64)> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    let root: Vec<u16> = format!("{drive}\\")
        .encode_utf16()
        .chain(iter::once(0))
        .collect();
    let mut free = 0u64;
    let mut total = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(root.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .ok()?;
    Some((total, free))
}

/// Bytes received and sent by every interface that is up, except loopback. Virtual adapters such as
/// VPNs are counted too, so their traffic can appear twice.
fn net_bytes() -> Option<NetBytes> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe { GetIfTable2(&mut table) }.ok().ok()?;
    // SAFETY: GetIfTable2 succeeded, so `table` holds `NumEntries` rows until FreeMibTable.
    let total = unsafe {
        let rows = std::slice::from_raw_parts(
            std::ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>(),
            (*table).NumEntries as usize,
        );
        rows.iter()
            .filter(|row| row.OperStatus == IfOperStatusUp && row.Type != IF_TYPE_SOFTWARE_LOOPBACK)
            .fold(NetBytes { received: 0, sent: 0 }, |sum, row| NetBytes {
                received: sum.received.saturating_add(row.InOctets),
                sent: sum.sent.saturating_add(row.OutOctets),
            })
    };
    unsafe { FreeMibTable(table.cast_const().cast()) };
    Some(total)
}

/// Tells the sampler whether the Stats page is on screen. Ignored while stats are off in config.
#[tauri::command]
pub fn stats_set_active(requests: State<'_, Requests>, active: bool) {
    let sender = requests.0.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = sender.send(active);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Instants `ms` after a fixed start, so the tests never read the clock.
    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Self(Instant::now())
        }

        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn cpu(idle: u64, total: u64) -> CpuTimes {
        CpuTimes { idle, total }
    }

    fn reading(cpu_idle: u64, cpu_total: u64, received: u64, sent: u64) -> Totals {
        Totals {
            cpu: Some(cpu(cpu_idle, cpu_total)),
            net: Some(NetBytes { received, sent }),
        }
    }

    // ---- CPU ----

    #[test]
    fn cpu_percent_is_the_busy_share_of_the_time_between_reads() {
        assert_eq!(cpu_percent(cpu(0, 0), cpu(75, 100)), Some(25.0));
        assert_eq!(cpu_percent(cpu(10, 100), cpu(10, 200)), Some(100.0));
        assert_eq!(cpu_percent(cpu(10, 100), cpu(60, 200)), Some(50.0));
    }

    #[test]
    fn cpu_percent_needs_time_to_have_passed() {
        assert_eq!(cpu_percent(cpu(5, 100), cpu(5, 100)), None);
    }

    #[test]
    fn cpu_percent_ignores_a_counter_that_went_back() {
        assert_eq!(cpu_percent(cpu(5, 100), cpu(5, 90)), None);
        assert_eq!(cpu_percent(cpu(50, 100), cpu(40, 200)), None);
    }

    #[test]
    fn cpu_percent_stays_inside_zero_and_one_hundred() {
        // Idle moving faster than the total can only read as fully idle.
        assert_eq!(cpu_percent(cpu(0, 0), cpu(150, 100)), Some(0.0));
    }

    // ---- Network ----

    #[test]
    fn rate_is_bytes_per_second_over_the_elapsed_time() {
        assert_eq!(rate(1_000, 3_000, Duration::from_secs(2)), Some(1_000.0));
        assert_eq!(rate(7, 7, Duration::from_secs(1)), Some(0.0));
    }

    #[test]
    fn rate_needs_elapsed_time() {
        assert_eq!(rate(0, 500, Duration::ZERO), None);
    }

    #[test]
    fn rate_ignores_a_counter_that_went_back() {
        assert_eq!(rate(9_000, 4_000, Duration::from_secs(1)), None);
    }

    // ---- Memory and disk ----

    #[test]
    fn used_percent_is_the_share_not_free() {
        assert_eq!(used_percent(200, 50), 75.0);
        assert_eq!(used_percent(200, 200), 0.0);
        assert_eq!(used_percent(200, 0), 100.0);
    }

    #[test]
    fn used_percent_of_an_unknown_total_is_zero_and_free_above_total_is_not_negative() {
        assert_eq!(used_percent(0, 0), 0.0);
        assert_eq!(used_percent(100, 500), 0.0);
    }

    #[test]
    fn snapshot_adds_memory_and_disk_to_the_rates() {
        let rates = Rates {
            cpu: Some(12.5),
            down: Some(2048.0),
            up: None,
        };
        let snapshot = Snapshot::new(rates, (16_000, 4_000), (500_000, 125_000));
        assert_eq!(snapshot.cpu, Some(12.5));
        assert_eq!(snapshot.memory.used, 12_000);
        assert_eq!(snapshot.memory.percent, 75.0);
        assert_eq!(snapshot.network.down, Some(2048.0));
        assert_eq!(snapshot.network.up, None);
        assert_eq!(snapshot.disk.free, 125_000);
        assert_eq!(snapshot.disk.percent, 75.0);
    }

    #[test]
    fn the_wire_format_names_missing_rates_as_null() {
        let snapshot = Snapshot::new(Rates::default(), (100, 50), (100, 50));
        let json = serde_json::to_value(snapshot).unwrap();
        assert!(json["cpu"].is_null());
        assert!(json["network"]["down"].is_null());
        assert_eq!(json["memory"]["percent"], 50.0);
        assert_eq!(json["disk"]["total"], 100);
    }

    // ---- Tracker: first read, deltas, smoothing, wraparound ----

    #[test]
    fn the_first_read_gives_no_rates() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        assert_eq!(
            tracker.observe(reading(0, 100, 0, 0), clock.at(0)),
            Rates::default()
        );
    }

    #[test]
    fn the_second_read_gives_cpu_and_network_rates() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        tracker.observe(reading(0, 100, 0, 0), clock.at(0));
        let rates = tracker.observe(reading(50, 200, 4_000, 1_000), clock.at(1_000));
        assert_eq!(rates.cpu, Some(50.0));
        assert_eq!(rates.down, Some(4_000.0));
        assert_eq!(rates.up, Some(1_000.0));
    }

    #[test]
    fn rates_are_smoothed_toward_each_new_reading() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        tracker.observe(reading(0, 100, 0, 0), clock.at(0));
        // First rate: 100 B/s, taken as is.
        assert_eq!(
            tracker.observe(reading(0, 200, 100, 0), clock.at(1_000)).down,
            Some(100.0)
        );
        // Next raw rate is 300 B/s. Halfway from 100 to 300 is 200.
        assert_eq!(
            tracker.observe(reading(0, 300, 400, 0), clock.at(2_000)).down,
            Some(200.0)
        );
    }

    #[test]
    fn a_counter_that_goes_back_gives_no_rate_for_that_read_only() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        tracker.observe(reading(0, 100, 5_000, 0), clock.at(0));
        tracker.observe(reading(0, 200, 6_000, 0), clock.at(1_000));
        // An adapter left, so the total dropped: no rate for this read.
        assert_eq!(
            tracker.observe(reading(0, 300, 2_000, 0), clock.at(2_000)).down,
            None
        );
        // The next read compares with the new total, and is not smoothed with the gap.
        assert_eq!(
            tracker.observe(reading(0, 400, 2_500, 0), clock.at(3_000)).down,
            Some(500.0)
        );
    }

    #[test]
    fn a_failed_counter_loses_only_its_own_rate() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        tracker.observe(reading(0, 100, 0, 0), clock.at(0));
        let rates = tracker.observe(
            Totals {
                cpu: None,
                net: Some(NetBytes {
                    received: 1_000,
                    sent: 0,
                }),
            },
            clock.at(1_000),
        );
        assert_eq!(rates.cpu, None);
        assert_eq!(rates.down, Some(1_000.0));
    }

    /// The Win32 reads on this machine. Nothing here is mocked, so this catches a bad struct size
    /// or a feature that is not compiled in.
    #[test]
    fn the_real_counters_read_on_this_machine() {
        let (total, free) = memory().expect("memory");
        assert!(total > 0 && free <= total);
        let (disk_total, disk_free) = disk().expect("system drive");
        assert!(disk_total > 0 && disk_free <= disk_total);
        assert!(cpu_times().expect("cpu").total > 0);
        assert!(net_bytes().is_some());
        assert!(sample(&mut Tracker::default(), Instant::now()).is_some());
    }

    #[test]
    fn reset_makes_the_next_read_silent_again() {
        let clock = Clock::new();
        let mut tracker = Tracker::default();
        tracker.observe(reading(0, 100, 0, 0), clock.at(0));
        tracker.observe(reading(0, 200, 100, 0), clock.at(1_000));
        tracker.reset();
        assert_eq!(
            tracker.observe(reading(0, 300, 200, 0), clock.at(2_000)),
            Rates::default()
        );
    }
}

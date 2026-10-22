//! System audio levels for the equalizer bars. WASAPI loopback on the default render endpoint
//! feeds `analysis`, and only four band levels leave the capture thread. Audio is analysed
//! locally and never stored or sent anywhere.
//!
//! Not a `Module`: nothing runs at startup. `audio_set_active` starts and stops the one
//! capture thread, which runs only while the bars are on screen.

mod analysis;

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::core::{IUnknown, Result as WinResult};

use analysis::{Analyzer, BAND_COUNT, FFT_SIZE, Gate, Sample, decode_mono, push_latest};

const LEVELS_EVENT: &str = "audio://levels";
/// One analysis frame, about 30 Hz. Also the wait between packet drains.
const TICK: Duration = Duration::from_millis(33);
/// Wait before reopening the device after a failure or a device change.
const RETRY: Duration = Duration::from_secs(1);
/// Shared-mode buffer length in 100 ns units (200 ms). This sets latency, not the frame rate.
const BUFFER_HNS: i64 = 2_000_000;

// WAVE format tags, and the Data1 field of the sub-format GUIDs (`KSDATAFORMAT_SUBTYPE_*`).
// The windows features for these are not enabled, so the values are written out here.
const FORMAT_PCM: u16 = 1;
const FORMAT_IEEE_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBFORMAT_PCM: u32 = 1;
const SUBFORMAT_IEEE_FLOAT: u32 = 3;

/// The frontend contract. Mirrors `audio://levels` in the frontend stores. Each value is 0..1.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelsUpdate {
    pub levels: [f32; BAND_COUNT],
}

/// The running capture thread, if there is one. Managed as app state for the command.
#[derive(Default)]
pub struct AudioController {
    capture: Mutex<Option<Capture>>,
}

struct Capture {
    stop: Sender<()>,
    thread: JoinHandle<()>,
}

/// Starts (`true`) or stops (`false`) the levels stream. The frontend calls this when the
/// compact pill shows the bars.
#[tauri::command]
pub async fn audio_set_active(app: AppHandle, active: bool) {
    set_active(&app, active);
}

/// Starts the capture thread, or does nothing when it is already running. Stopping joins the
/// thread, which exits within one frame.
pub fn set_active(app: &AppHandle, active: bool) {
    let Some(controller) = app.try_state::<AudioController>() else {
        return;
    };
    let mut capture = controller
        .capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if active {
        if capture.is_some() {
            return;
        }
        let (stop, inbox) = mpsc::channel();
        let app = app.clone();
        let emit = move |levels| {
            let _ = app.emit(LEVELS_EVENT, LevelsUpdate { levels });
        };
        if let Ok(thread) = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || run(inbox, emit))
        {
            *capture = Some(Capture { stop, thread });
        }
    } else if let Some(running) = capture.take() {
        // Dropping the sender disconnects the channel, which wakes the thread at once.
        drop(running.stop);
        let _ = running.thread.join();
    }
}

/// Thread body. Captures and emits until `stop` disconnects. A failed open or a lost device
/// waits `RETRY` and tries again, and the wait also watches `stop`.
fn run(stop: Receiver<()>, mut emit: impl FnMut([f32; BAND_COUNT])) {
    // Dedicated thread with no message pump, so MTA is the right apartment.
    let Some(_com) = ComGuard::init() else {
        return;
    };
    loop {
        if let Some(stream) = Stream::open()
            && capture(&stream, &stop, &mut emit) == Ended::Stopped
        {
            return;
        }
        if !matches!(stop.recv_timeout(RETRY), Err(RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

#[derive(PartialEq)]
enum Ended {
    Stopped,
    /// Any WASAPI error. A removed or changed device reports `AUDCLNT_E_DEVICE_INVALIDATED`,
    /// and every other error is treated the same way: reopen after `RETRY`.
    Lost,
}

/// Pulls packets once per tick and emits the levels that changed. An idle output sends no
/// packets at all, so an empty tick counts as silence.
fn capture(
    stream: &Stream,
    stop: &Receiver<()>,
    emit: &mut impl FnMut([f32; BAND_COUNT]),
) -> Ended {
    if unsafe { stream.client.Start() }.is_err() {
        return Ended::Lost;
    }
    let mut analyzer = Analyzer::new(stream.format.sample_rate);
    let mut gate = Gate::default();
    let mut window = VecDeque::with_capacity(FFT_SIZE);
    let mut fresh = Vec::new();
    let silent_tick = stream.format.sample_rate as usize * TICK.as_millis() as usize / 1_000;
    loop {
        if !matches!(stop.recv_timeout(TICK), Err(RecvTimeoutError::Timeout)) {
            if let Some(levels) = gate.settle() {
                emit(levels);
            }
            return Ended::Stopped;
        }
        fresh.clear();
        if stream.drain(&mut fresh).is_err() {
            return Ended::Lost;
        }
        if fresh.is_empty() {
            fresh.resize(silent_tick, 0.0);
        }
        push_latest(&mut window, &fresh);
        if let Some(levels) = gate.decide(analyzer.analyze(window.make_contiguous())) {
            emit(levels);
        }
    }
}

/// An open loopback capture on the default render endpoint, in its shared mix format.
/// Loopback taps the mix before the device, so it hears any app's output.
struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: Format,
}

impl Stream {
    fn open() -> Option<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).ok()?;
            let mix = client.GetMixFormat().ok()?;
            let format = read_format(mix);
            let initialized = format.is_some()
                && client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_LOOPBACK,
                        BUFFER_HNS,
                        0,
                        mix,
                        None,
                    )
                    .is_ok();
            CoTaskMemFree(Some(mix as *const c_void));
            if !initialized {
                return None;
            }
            let capture = client.GetService::<IAudioCaptureClient>().ok()?;
            Some(Self {
                client,
                capture,
                format: format?,
            })
        }
    }

    /// Moves every queued packet into `out` as mono samples. Silent packets become zeros.
    fn drain(&self, out: &mut Vec<f32>) -> WinResult<()> {
        let frame_bytes = self.format.frame_bytes();
        unsafe {
            let mut next = self.capture.GetNextPacketSize()?;
            while next > 0 {
                let mut data: *mut u8 = ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                if (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    out.resize(out.len() + frames as usize, 0.0);
                } else if frames > 0 {
                    let bytes = slice::from_raw_parts(data, frames as usize * frame_bytes);
                    decode_mono(bytes, self.format.sample, self.format.channels, out);
                }
                self.capture.ReleaseBuffer(frames)?;
                next = self.capture.GetNextPacketSize()?;
            }
        }
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

/// The captured stream's shape: sample rate, channel count and sample type.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Format {
    sample_rate: u32,
    channels: usize,
    sample: Sample,
}

impl Format {
    fn frame_bytes(self) -> usize {
        self.channels * self.sample.width()
    }
}

/// # Safety
/// `mix` must point to a format returned by `GetMixFormat`.
unsafe fn read_format(mix: *const WAVEFORMATEX) -> Option<Format> {
    let base = unsafe { *mix };
    let sub_format = (base.wFormatTag == FORMAT_EXTENSIBLE && base.cbSize >= 22)
        .then(|| unsafe { (*mix.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat.data1 });
    parse_format(
        base.wFormatTag,
        base.nChannels,
        base.nSamplesPerSec,
        base.wBitsPerSample,
        sub_format,
    )
}

/// Pure form of `read_format`. `sub_format` is the Data1 of an extensible format's sub-format
/// GUID. Only 16-bit PCM and 32-bit float are accepted, and anything else means no levels.
fn parse_format(
    tag: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    sub_format: Option<u32>,
) -> Option<Format> {
    let sample = match (tag, sub_format, bits) {
        (FORMAT_IEEE_FLOAT, _, 32) | (FORMAT_EXTENSIBLE, Some(SUBFORMAT_IEEE_FLOAT), 32) => {
            Sample::F32
        }
        (FORMAT_PCM, _, 16) | (FORMAT_EXTENSIBLE, Some(SUBFORMAT_PCM), 16) => Sample::I16,
        _ => return None,
    };
    (channels > 0 && sample_rate > 0).then_some(Format {
        sample_rate,
        channels: usize::from(channels),
        sample,
    })
}

/// CoInitializeEx must be paired with CoUninitialize on the same thread. Only the success path
/// (including S_FALSE) increments the count, so only that path gets undone.
struct ComGuard;

impl ComGuard {
    fn init() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_float_and_pcm16_in_plain_and_extensible_form() {
        assert_eq!(
            parse_format(FORMAT_IEEE_FLOAT, 2, 48_000, 32, None),
            Some(Format {
                sample_rate: 48_000,
                channels: 2,
                sample: Sample::F32,
            })
        );
        assert_eq!(
            parse_format(FORMAT_PCM, 1, 44_100, 16, None).map(|f| f.sample),
            Some(Sample::I16)
        );
        assert_eq!(
            parse_format(FORMAT_EXTENSIBLE, 2, 48_000, 32, Some(SUBFORMAT_IEEE_FLOAT))
                .map(|f| f.sample),
            Some(Sample::F32)
        );
        assert_eq!(
            parse_format(FORMAT_EXTENSIBLE, 2, 48_000, 16, Some(SUBFORMAT_PCM)).map(|f| f.sample),
            Some(Sample::I16)
        );
    }

    #[test]
    fn rejects_other_layouts_and_empty_streams() {
        assert_eq!(parse_format(FORMAT_PCM, 2, 48_000, 24, None), None);
        assert_eq!(
            parse_format(FORMAT_EXTENSIBLE, 2, 48_000, 32, Some(SUBFORMAT_PCM)),
            None
        );
        assert_eq!(parse_format(FORMAT_IEEE_FLOAT, 0, 48_000, 32, None), None);
        assert_eq!(parse_format(FORMAT_IEEE_FLOAT, 2, 0, 32, None), None);
    }

    #[test]
    fn frame_bytes_follow_channels_and_sample_width() {
        let format = Format {
            sample_rate: 48_000,
            channels: 2,
            sample: Sample::I16,
        };
        assert_eq!(format.frame_bytes(), 4);
    }
}

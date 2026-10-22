//! Signal analysis for the audio-reactive bars. Pure functions over mono `f32` samples, so
//! nothing here touches Windows and every step is unit tested.
//!
//! Per frame, the newest `FFT_SIZE` samples are Hann-windowed and transformed, the power in
//! four log-spaced bands is summed, the power is mapped through dB onto 0..1, and the result
//! is smoothed. `Gate` then decides which frames reach the frontend.

use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::ops::Range;

/// Samples per analysis frame. At 48 kHz that is about 21 ms of audio.
pub const FFT_SIZE: usize = 1024;
pub const BAND_COUNT: usize = 4;
/// Band edges in Hz. Band `i` covers `EDGES_HZ[i]..EDGES_HZ[i + 1]`.
const EDGES_HZ: [f32; BAND_COUNT + 1] = [40.0, 250.0, 1_000.0, 4_000.0, 12_000.0];
/// Band power in dB that reads as 0 and as 1.
const DB_FLOOR: f32 = -60.0;
const DB_CEILING: f32 = -10.0;
/// Fraction of the remaining distance to the target covered per frame, rising and falling.
const ATTACK: f32 = 0.6;
const RELEASE: f32 = 0.15;
/// Levels below this are silence. They snap to 0 and count as quiet.
const SILENCE: f32 = 0.01;
/// Smallest change in any band that is worth sending to the frontend.
const CHANGE: f32 = 0.005;

/// The sample layouts the capture accepts. Both are converted to `f32` in -1..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    F32,
    I16,
}

impl Sample {
    pub fn width(self) -> usize {
        match self {
            Sample::F32 => 4,
            Sample::I16 => 2,
        }
    }

    /// Reads one sample. A slice of the wrong length reads as silence rather than panicking.
    fn read(self, bytes: &[u8]) -> f32 {
        match (self, bytes) {
            (Sample::F32, &[a, b, c, d]) => f32::from_le_bytes([a, b, c, d]),
            (Sample::I16, &[a, b]) => f32::from(i16::from_le_bytes([a, b])) / 32_768.0,
            _ => 0.0,
        }
    }
}

/// Appends the mono mix of interleaved frames to `out`. Channels are averaged, and a
/// trailing partial frame is ignored.
pub fn decode_mono(bytes: &[u8], sample: Sample, channels: usize, out: &mut Vec<f32>) {
    if channels == 0 {
        return;
    }
    let width = sample.width();
    out.extend(bytes.chunks_exact(width * channels).map(|frame| {
        let sum: f32 = frame.chunks_exact(width).map(|s| sample.read(s)).sum();
        sum / channels as f32
    }));
}

/// Appends `samples` to the rolling window, keeping only the newest `FFT_SIZE`.
pub fn push_latest(window: &mut VecDeque<f32>, samples: &[f32]) {
    window.extend(samples);
    let excess = window.len().saturating_sub(FFT_SIZE);
    window.drain(..excess);
}

/// FFT bins for each band at `sample_rate`. Bin `k` is centred on `k * sample_rate / FFT_SIZE`.
/// Bin 0 (DC) is never used.
pub fn band_bins(sample_rate: u32) -> [Range<usize>; BAND_COUNT] {
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    let nyquist_bin = FFT_SIZE / 2;
    std::array::from_fn(|band| {
        let first = ((EDGES_HZ[band] / bin_hz).ceil() as usize).max(1);
        let end = ((EDGES_HZ[band + 1] / bin_hz).ceil() as usize).min(nyquist_bin + 1);
        first..end.max(first)
    })
}

/// Maps band power onto 0..1 through dB: `DB_FLOOR` reads as 0 and `DB_CEILING` as 1.
/// Zero power is far below the floor, so it reads as 0.
pub fn power_to_level(power: f32) -> f32 {
    let db = 10.0 * (power + 1e-12).log10();
    let level = (db - DB_FLOOR) / (DB_CEILING - DB_FLOOR);
    if level.is_nan() {
        0.0
    } else {
        level.clamp(0.0, 1.0)
    }
}

/// Moves `current` toward `target`, quickly up and slowly down. Results below `SILENCE` snap to 0.
pub fn smooth(current: f32, target: f32) -> f32 {
    let rate = if target > current { ATTACK } else { RELEASE };
    let next = current + (target - current) * rate;
    if next < SILENCE { 0.0 } else { next }
}

/// Turns the newest audio into smoothed band levels. Owns the window tables and the smoothing state.
pub struct Analyzer {
    window: Vec<f32>,
    twiddles: Vec<(f32, f32)>,
    bands: [Range<usize>; BAND_COUNT],
    smoothed: [f32; BAND_COUNT],
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            window: (0..FFT_SIZE)
                .map(|n| 0.5 - 0.5 * (TAU * n as f32 / FFT_SIZE as f32).cos())
                .collect(),
            twiddles: (0..FFT_SIZE / 2)
                .map(|k| {
                    let angle = -TAU * k as f32 / FFT_SIZE as f32;
                    (angle.cos(), angle.sin())
                })
                .collect(),
            bands: band_bins(sample_rate),
            smoothed: [0.0; BAND_COUNT],
        }
    }

    /// Raw power per band for the newest `FFT_SIZE` samples. Shorter input is padded with
    /// silence at the front.
    pub fn band_powers(&self, samples: &[f32]) -> [f32; BAND_COUNT] {
        let mut re = [0.0; FFT_SIZE];
        let mut im = [0.0; FFT_SIZE];
        let newest = &samples[samples.len().saturating_sub(FFT_SIZE)..];
        let start = FFT_SIZE - newest.len();
        for (offset, sample) in newest.iter().enumerate() {
            re[start + offset] = sample * self.window[start + offset];
        }
        fft(&mut re, &mut im, &self.twiddles);

        // A Hann window has coherent gain 0.5, so this scale makes a sine of amplitude A at a
        // bin centre read as A, and a full-scale sine reads about 0 dB.
        let scale = (4.0 / FFT_SIZE as f32).powi(2);
        let mut powers = [0.0; BAND_COUNT];
        for (power, range) in powers.iter_mut().zip(&self.bands) {
            *power = range
                .clone()
                .map(|k| re[k].powi(2) + im[k].powi(2))
                .sum::<f32>()
                * scale;
        }
        powers
    }

    /// Smoothed levels for the newest samples, one per band.
    pub fn analyze(&mut self, samples: &[f32]) -> [f32; BAND_COUNT] {
        let powers = self.band_powers(samples);
        for (smoothed, power) in self.smoothed.iter_mut().zip(powers) {
            *smoothed = smooth(*smoothed, power_to_level(power));
        }
        self.smoothed
    }
}

/// In-place iterative radix-2 FFT. `re` and `im` hold `FFT_SIZE` values, and `twiddles` holds
/// `e^(-2 pi i k / N)` for the first half of the circle.
fn fft(re: &mut [f32], im: &mut [f32], twiddles: &[(f32, f32)]) {
    let n = re.len();
    // Reorder into bit-reversed order so every butterfly stage runs in place.
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let stride = n / len;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let (wr, wi) = twiddles[k * stride];
                let (a, b) = (start + k, start + k + half);
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Decides which frames reach the frontend. A frame is sent only when some band moved by
/// `CHANGE`. Silence is sent once, as a single zero frame, so the bars settle at rest.
pub struct Gate {
    last: [f32; BAND_COUNT],
    /// True when the frontend holds a zero frame, or has never been sent a frame.
    at_rest: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            last: [0.0; BAND_COUNT],
            at_rest: true,
        }
    }
}

impl Gate {
    pub fn decide(&mut self, levels: [f32; BAND_COUNT]) -> Option<[f32; BAND_COUNT]> {
        if levels.iter().all(|level| *level < SILENCE) {
            return self.settle();
        }
        self.at_rest = false;
        if !moved(&self.last, &levels) {
            return None;
        }
        self.last = levels;
        Some(levels)
    }

    /// The frame that leaves the bars at rest. Sent only when the frontend still holds a
    /// non-zero frame, so stopping costs at most one frame.
    pub fn settle(&mut self) -> Option<[f32; BAND_COUNT]> {
        if self.at_rest {
            return None;
        }
        self.at_rest = true;
        self.last = [0.0; BAND_COUNT];
        Some(self.last)
    }
}

fn moved(a: &[f32; BAND_COUNT], b: &[f32; BAND_COUNT]) -> bool {
    a.iter().zip(b).any(|(x, y)| (x - y).abs() >= CHANGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, amplitude: f32, rate: u32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|n| amplitude * (TAU * hz * n as f32 / rate as f32).sin())
            .collect()
    }

    fn loudest(powers: &[f32; BAND_COUNT]) -> usize {
        powers
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    #[test]
    fn bands_are_scaled_to_the_sample_rate() {
        assert_eq!(band_bins(RATE), [1..6, 6..22, 22..86, 86..256]);
    }

    #[test]
    fn every_band_has_bins_at_common_rates() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            for range in band_bins(rate) {
                assert!(!range.is_empty(), "empty band at {rate} Hz");
            }
        }
    }

    #[test]
    fn tones_land_in_their_band() {
        let analyzer = Analyzer::new(RATE);
        // 1 kHz sits on the edge between bands 1 and 2, so the check uses 1.5 kHz.
        for (hz, band) in [(100.0, 0), (1_500.0, 2), (6_000.0, 3)] {
            let samples = sine(hz, 0.5, RATE, FFT_SIZE);
            let powers = analyzer.band_powers(&samples);
            assert_eq!(loudest(&powers), band, "{hz} Hz powers {powers:?}");
        }
    }

    #[test]
    fn silence_reads_as_zero() {
        let mut analyzer = Analyzer::new(RATE);
        assert_eq!(analyzer.analyze(&[0.0; FFT_SIZE]), [0.0; BAND_COUNT]);
    }

    #[test]
    fn power_maps_minus_sixty_to_minus_ten_db_onto_the_scale() {
        assert_eq!(power_to_level(0.0), 0.0);
        assert!(power_to_level(1e-6) < 0.01); // -60 dB
        assert!((power_to_level(1e-5) - 0.2).abs() < 0.01); // -50 dB
        assert!((power_to_level(0.01) - 0.8).abs() < 0.01); // -20 dB
        assert_eq!(power_to_level(1.0), 1.0); // 0 dB clamps
        assert_eq!(power_to_level(f32::NAN), 0.0);
    }

    #[test]
    fn smoothing_attacks_fast_and_releases_slowly() {
        assert!((smooth(0.0, 1.0) - 0.6).abs() < 1e-6);
        assert!((smooth(1.0, 0.0) - 0.85).abs() < 1e-6);
    }

    #[test]
    fn smoothing_snaps_small_values_to_zero() {
        assert_eq!(smooth(0.005, 0.0), 0.0);
        assert!(smooth(0.012, 0.0) > 0.0);
    }

    #[test]
    fn stereo_float_mixes_down_to_the_average() {
        let bytes: Vec<u8> = [1.0f32, 0.0, -1.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut mono = Vec::new();
        decode_mono(&bytes, Sample::F32, 2, &mut mono);
        assert_eq!(mono, [0.5, -0.5]);
    }

    #[test]
    fn pcm16_is_scaled_to_unit_range() {
        let bytes = [i16::MAX.to_le_bytes(), i16::MIN.to_le_bytes()].concat();
        let mut mono = Vec::new();
        decode_mono(&bytes, Sample::I16, 1, &mut mono);
        assert!((mono[0] - 1.0).abs() < 1e-4);
        assert_eq!(mono[1], -1.0);
    }

    #[test]
    fn partial_trailing_frame_is_ignored() {
        let mut mono = Vec::new();
        decode_mono(&[0u8; 6], Sample::I16, 2, &mut mono);
        assert_eq!(mono.len(), 1);
    }

    #[test]
    fn window_keeps_only_the_newest_samples() {
        let mut window = VecDeque::new();
        let samples: Vec<f32> = (0..1_500).map(|n| n as f32).collect();
        push_latest(&mut window, &samples);
        assert_eq!(window.len(), FFT_SIZE);
        assert_eq!(window.front(), Some(&476.0));
        assert_eq!(window.back(), Some(&1_499.0));
    }

    #[test]
    fn gate_stays_quiet_until_sound_arrives() {
        let mut gate = Gate::default();
        assert_eq!(gate.decide([0.0; BAND_COUNT]), None);
        assert_eq!(gate.settle(), None);
    }

    #[test]
    fn gate_sends_only_real_movement() {
        let mut gate = Gate::default();
        assert_eq!(
            gate.decide([0.5, 0.0, 0.0, 0.0]),
            Some([0.5, 0.0, 0.0, 0.0])
        );
        assert_eq!(gate.decide([0.5, 0.0, 0.0, 0.0]), None);
        assert_eq!(gate.decide([0.502, 0.0, 0.0, 0.0]), None);
        assert_eq!(
            gate.decide([0.6, 0.0, 0.0, 0.0]),
            Some([0.6, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn gate_sends_one_zero_frame_when_silence_starts() {
        let mut gate = Gate::default();
        gate.decide([0.5, 0.5, 0.5, 0.5]);
        assert_eq!(gate.decide([0.0; BAND_COUNT]), Some([0.0; BAND_COUNT]));
        assert_eq!(gate.decide([0.0; BAND_COUNT]), None);
    }

    #[test]
    fn settle_sends_zeros_only_when_the_frontend_holds_sound() {
        let mut gate = Gate::default();
        gate.decide([0.3, 0.0, 0.0, 0.0]);
        assert_eq!(gate.settle(), Some([0.0; BAND_COUNT]));
        assert_eq!(gate.settle(), None);
    }
}

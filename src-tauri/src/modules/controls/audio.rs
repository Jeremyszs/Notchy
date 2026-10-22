//! Volume, output mute and microphone mute, through the default Core Audio endpoints.

use std::ptr;

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::IUnknown;

use super::{Control, Kind, Reading, error_text, flag, is_on, percent};

const NO_OUTPUT: &str = "No audio output";
const NO_MICROPHONE: &str = "No microphone";

/// The endpoint volume interface of the default device for `flow`.
fn endpoint(flow: EDataFlow, missing: &str) -> Result<IAudioEndpointVolume, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL)
                .map_err(|error| error_text(&error))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(flow, eConsole)
            .map_err(|_| missing.to_owned())?;
        device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|error| error_text(&error))
    }
}

pub(super) struct Volume;

impl Control for Volume {
    fn id(&self) -> &'static str {
        "volume"
    }

    fn label(&self) -> &'static str {
        "Volume"
    }

    fn kind(&self) -> Kind {
        Kind::Slider
    }

    fn read(&self) -> Result<Reading, String> {
        let volume = endpoint(eRender, NO_OUTPUT)?;
        let scalar =
            unsafe { volume.GetMasterVolumeLevelScalar() }.map_err(|error| error_text(&error))?;
        Ok(Reading {
            value: percent(f64::from(scalar) * 100.0),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let volume = endpoint(eRender, NO_OUTPUT)?;
        let scalar = (percent(value) / 100.0) as f32;
        unsafe { volume.SetMasterVolumeLevelScalar(scalar, ptr::null()) }
            .map_err(|error| error_text(&error))
    }
}

pub(super) struct Mute;

impl Control for Mute {
    fn id(&self) -> &'static str {
        "mute"
    }

    fn label(&self) -> &'static str {
        "Mute"
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        let volume = endpoint(eRender, NO_OUTPUT)?;
        let muted = unsafe { volume.GetMute() }.map_err(|error| error_text(&error))?;
        Ok(Reading {
            value: flag(muted.as_bool()),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let volume = endpoint(eRender, NO_OUTPUT)?;
        unsafe { volume.SetMute(is_on(value), ptr::null()) }.map_err(|error| error_text(&error))
    }
}

/// Microphone "live" is the inverse of the capture endpoint's mute flag.
pub(super) struct Mic;

impl Control for Mic {
    fn id(&self) -> &'static str {
        "mic"
    }

    fn label(&self) -> &'static str {
        "Microphone"
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        let microphone = endpoint(eCapture, NO_MICROPHONE)?;
        let muted = unsafe { microphone.GetMute() }.map_err(|error| error_text(&error))?;
        Ok(Reading {
            value: flag(!muted.as_bool()),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let microphone = endpoint(eCapture, NO_MICROPHONE)?;
        unsafe { microphone.SetMute(!is_on(value), ptr::null()) }
            .map_err(|error| error_text(&error))
    }
}

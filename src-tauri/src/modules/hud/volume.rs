//! Output volume and mute, from Core Audio's endpoint notifications. Windows calls back on every
//! change, whatever caused it: the volume keys, the Settings app, another app, or the output
//! device changing. The callback runs on a Core Audio thread and only forwards a `HudUpdate`.
//!
//! A worker thread owns the COM objects. It re-attaches to the new default output whenever the
//! default changes, so the HUD follows a pair of headphones being plugged in.

use std::sync::mpsc::{self, Sender};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{IUnknown, PCWSTR, Result as WinResult};
use windows_core::implement;

use super::{HudUpdate, Kind, OWN_CONTEXT, level_from_scalar};
use crate::modules::controls::ComGuard;

/// Thread body. Runs for the life of the app.
pub(super) fn run(updates: Sender<HudUpdate>) {
    // Dedicated thread with no message pump, so MTA is the right apartment.
    let Some(_com) = ComGuard::init() else {
        return;
    };
    let Ok(enumerator) = (unsafe {
        CoCreateInstance::<_, IMMDeviceEnumerator>(
            &MMDeviceEnumerator,
            None::<&IUnknown>,
            CLSCTX_ALL,
        )
    }) else {
        return;
    };

    let (default_changed, changes) = mpsc::channel::<()>();
    let watch: IMMNotificationClient = OutputWatch {
        changed: default_changed,
    }
    .into();
    // Without the registration the HUD still shows changes on the output it attached to first.
    let _ = unsafe { enumerator.RegisterEndpointNotificationCallback(&watch) };

    let mut attached = attach(&enumerator, &updates);
    while changes.recv().is_ok() {
        if let Some(old) = attached.take() {
            old.detach();
        }
        attached = attach(&enumerator, &updates);
    }
}

/// A callback registered on one endpoint, and the endpoint it belongs to.
struct Attached {
    volume: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

impl Attached {
    fn detach(self) {
        let _ = unsafe { self.volume.UnregisterControlChangeNotify(&self.callback) };
    }
}

/// Registers the volume callback on the default output, if there is one.
fn attach(enumerator: &IMMDeviceEnumerator, updates: &Sender<HudUpdate>) -> Option<Attached> {
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.ok()?;
    let volume = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.ok()?;
    let callback: IAudioEndpointVolumeCallback = VolumeWatch {
        updates: updates.clone(),
    }
    .into();
    unsafe { volume.RegisterControlChangeNotify(&callback) }.ok()?;
    Some(Attached { volume, callback })
}

/// Forwards each volume or mute change on the endpoint. Changes that Notchy made itself carry
/// `OWN_CONTEXT` and are dropped, so the panel's slider does not raise the HUD.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeWatch {
    updates: Sender<HudUpdate>,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeWatch_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> WinResult<()> {
        // Core Audio passes a pointer that is valid for the duration of this call.
        if let Some(data) = unsafe { data.as_ref() }
            && data.guidEventContext != OWN_CONTEXT
        {
            let _ = self.updates.send(HudUpdate {
                kind: Kind::Volume,
                level: level_from_scalar(data.fMasterVolume),
                muted: data.bMuted.as_bool(),
            });
        }
        Ok(())
    }
}

/// Reports when the default output changes. Every other device event is ignored.
#[implement(IMMNotificationClient)]
struct OutputWatch {
    changed: Sender<()>,
}

impl IMMNotificationClient_Impl for OutputWatch_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> WinResult<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> WinResult<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> WinResult<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> WinResult<()> {
        if flow == eRender && role == eConsole {
            let _ = self.changed.send(());
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> WinResult<()> {
        Ok(())
    }
}

//! Drags a shelf item into other apps as a native OLE drag. A file carries its stored path as
//! CF_HDROP, and text carries the full stored text as CF_UNICODETEXT. Both are copies: the drag
//! never moves or deletes anything. Runs on the main thread, which Tauri and the catcher already
//! use for OLE.

use std::iter::once;
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
    DV_E_FORMATETC, E_NOTIMPL, E_OUTOFMEMORY, GlobalFree, HGLOBAL, OLE_E_ADVISENOTSUPPORTED,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DoDragDrop, IDropSource, IDropSource_Impl,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;
use windows::core::{BOOL as ComBool, HRESULT, Ref, Result, implement};

const CF_UNICODETEXT: u16 = 13;

/// Set while this process drags its own item. The catcher checks it, so the shelf never takes back
/// what it just gave out.
pub static OWN_DRAG: AtomicBool = AtomicBool::new(false);

pub enum Payload {
    File(PathBuf),
    Text(String),
}

/// Runs one drag to completion on the calling thread, which must own OLE. Returns when the button
/// is released or the drag is cancelled.
pub fn run(payload: Payload) -> std::result::Result<(), String> {
    let source: IDropSource = Source.into();
    let data: IDataObject = Data { payload }.into();
    let mut effect = DROPEFFECT_COPY;
    OWN_DRAG.store(true, Ordering::SeqCst);
    let outcome = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    OWN_DRAG.store(false, Ordering::SeqCst);
    outcome.ok().map_err(|_| "Drag failed".to_string())
}

fn left_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 }
}

/// The standard drag loop: Escape cancels, releasing the button drops, and anything else goes on.
#[implement(IDropSource)]
struct Source;

#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: ComBool, _keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if !left_button_down() {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// The one format the payload offers, as a fresh HGLOBAL for each request.
#[implement(IDataObject)]
struct Data {
    payload: Payload,
}

impl Data {
    fn format(&self) -> u16 {
        match self.payload {
            Payload::File(_) => CF_HDROP.0,
            Payload::Text(_) => CF_UNICODETEXT,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match &self.payload {
            Payload::File(path) => {
                // DROPFILES: list offset, drop point (unused), non-client flag (no), wide strings (yes).
                let mut bytes = Vec::new();
                bytes.extend(20u32.to_le_bytes());
                bytes.extend([0u8; 8]);
                bytes.extend(0u32.to_le_bytes());
                bytes.extend(1u32.to_le_bytes());
                for unit in path.as_os_str().encode_wide().chain(once(0)).chain(once(0)) {
                    bytes.extend(unit.to_le_bytes());
                }
                bytes
            }
            Payload::Text(text) => text
                .encode_utf16()
                .chain(once(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }
}

fn wants(format: *const FORMATETC, wanted: u16) -> bool {
    if format.is_null() {
        return false;
    }
    let format = unsafe { &*format };
    format.cfFormat == wanted
        && format.dwAspect == DVASPECT_CONTENT.0
        && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
}

/// Copies bytes into a movable global block. The receiver of the medium frees it.
fn global(bytes: &[u8]) -> Result<HGLOBAL> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let ptr = GlobalLock(memory).cast::<u8>();
        if ptr.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err(E_OUTOFMEMORY.into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(memory);
        Ok(HGLOBAL(memory.0))
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for Data_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        if !wants(pformatetcin, self.format()) {
            return Err(DV_E_FORMATETC.into());
        }
        let memory = global(&self.bytes())?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: memory },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if wants(pformatetc, self.format()) {
            HRESULT(0)
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: ComBool) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        let format = FORMATETC {
            cfFormat: self.format(),
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe { SHCreateStdEnumFmtEtc(&[format]) }
    }

    fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: Ref<IAdviseSink>) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_payload_is_a_double_terminated_wide_list_after_dropfiles() {
        let data = Data { payload: Payload::File(PathBuf::from(r"C:\a\b.txt")) };
        let bytes = data.bytes();
        assert_eq!(&bytes[0..4], &20u32.to_le_bytes());
        assert_eq!(&bytes[16..20], &1u32.to_le_bytes());
        let units: Vec<u16> = bytes[20..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(&units[units.len() - 2..], &[0, 0]);
        let path = String::from_utf16(&units[..units.len() - 2]).unwrap();
        assert_eq!(path, r"C:\a\b.txt");
    }

    #[test]
    fn text_payload_is_the_full_text_as_nul_terminated_utf16() {
        let text = "héllo 😀";
        let data = Data { payload: Payload::Text(text.to_string()) };
        let bytes = data.bytes();
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(units.last(), Some(&0));
        assert_eq!(String::from_utf16(&units[..units.len() - 1]).unwrap(), text);
    }

    #[test]
    fn each_payload_offers_only_its_own_format() {
        let file = Data { payload: Payload::File(PathBuf::from("x")) };
        let text = Data { payload: Payload::Text("x".into()) };
        assert_eq!(file.format(), CF_HDROP.0);
        assert_eq!(text.format(), CF_UNICODETEXT);
    }
}

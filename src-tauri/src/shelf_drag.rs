//! Native Windows drag-out: lets the user drag shelf items out of the
//! notch into Explorer, folders or any app that accepts files.
//!
//! Flow: build a shell data object (CF_HDROP) from the paths, pair it with
//! an IDropSource, and enter the OLE drag loop (DoDragDrop) on a dedicated
//! STA thread. The call blocks until the user drops or cancels; the
//! returned effect tells us whether the item moved out.

#![cfg(windows)]

use std::path::Path;
use std::time::Instant;

use windows::core::{implement, BOOL, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, IDataObject, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK,
    DROPEFFECT_MOVE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{ILFindLastID, SHCreateDataObject, SHParseDisplayName};

#[derive(Debug)]
pub enum DragEffect {
    Move,
    Copy,
    Link,
    None,
}

/// Enters the OLE drag loop carrying `paths`. Blocks until the user drops
/// or cancels. Must not be called from the main thread.
pub fn do_file_drag(paths: &[String]) -> Result<DragEffect, String> {
    if paths.is_empty() {
        return Err("no files to drag".into());
    }
    unsafe {
        // OLE requires an STA thread for DoDragDrop.
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| format!("CoInitializeEx: {e}"))?;

        let result = run_drag(paths);
        eprintln!("[shelf] drag loop done: {result:?}");
        let _ = CoTaskMemFree(None);
        result
    }
}

unsafe fn run_drag(paths: &[String]) -> Result<DragEffect, String> {
    let data_object: IDataObject =
        create_hdrop_data_object(paths).map_err(|e| e.to_string())?;

    let drop_source: IDropSource = DropSource::default().into();

    let mut effect = DROPEFFECT(0);
    let hr = DoDragDrop(
        &data_object,
        &drop_source,
        DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0 | DROPEFFECT_LINK.0),
        &mut effect,
    );
    if hr.is_err() && hr != DRAGDROP_S_DROP && hr != DRAGDROP_S_CANCEL {
        return Err(format!("DoDragDrop: {hr:?}"));
    }

    Ok(if (effect.0 & DROPEFFECT_MOVE.0) != 0 {
        DragEffect::Move
    } else if (effect.0 & DROPEFFECT_COPY.0) != 0 {
        DragEffect::Copy
    } else if (effect.0 & DROPEFFECT_LINK.0) != 0 {
        DragEffect::Link
    } else {
        DragEffect::None
    })
}

unsafe fn create_hdrop_data_object(paths: &[String]) -> Result<IDataObject, String> {
    let parent = Path::new(&paths[0])
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();

    let full = Path::new(&paths[0])
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();

    let mut folder_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
    SHParseDisplayName(PCWSTR(parent.as_ptr()), None, &mut folder_pidl, 0, None)
        .map_err(|e| format!("SHParseDisplayName(folder): {e}"))?;

    let mut full_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
    SHParseDisplayName(PCWSTR(full.as_ptr()), None, &mut full_pidl, 0, None)
        .map_err(|e| format!("SHParseDisplayName(file): {e}"))?;

    // The last SHITEMID of the absolute pidl is the child-relative pidl.
    let child = ILFindLastID(full_pidl);

    let data: IDataObject = SHCreateDataObject(
        Some(folder_pidl),
        Some([child as *const ITEMIDLIST].as_slice()),
        None,
    )
    .map_err(|e| format!("SHCreateDataObject: {e}"))?;

    CoTaskMemFree(Some(folder_pidl as *const core::ffi::c_void));
    CoTaskMemFree(Some(full_pidl as *const core::ffi::c_void));

    Ok(data)
}

#[implement(IDropSource)]
struct DropSource {
    start: Instant,
}

impl Default for DropSource {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        // Mouse released (and the press had time to register): drop.
        if !grfkeystate.contains(MODIFIERKEYS_FLAGS(1u32))
            && self.start.elapsed() > std::time::Duration::from_millis(250)
        {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

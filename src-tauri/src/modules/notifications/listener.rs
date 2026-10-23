//! Reads toasts through the Windows user notification listener. Unpackaged apps can use it once
//! the user allows notification access in Windows. Only this file touches WinRT. It returns plain
//! data, and `notifications.rs` decides what to show.
//!
//! The listener's change event fails for unpackaged apps (0x80070490), so toasts are read by
//! polling `GetNotificationsAsync`. Icons and click actions are not read.

use std::future::IntoFuture;

use tauri::async_runtime::block_on;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{NotificationKinds, UserNotification};
use windows::core::Result as WinResult;

use super::Access;

/// One toast as the listener reports it. The text is the toast's own lines, in order.
pub struct Toast {
    pub id: u32,
    /// The app's AppUserModelId. The key that `deniedApps` uses.
    pub app_id: String,
    /// The app's display name, for the label. Falls back to the id.
    pub app_name: String,
    pub lines: Vec<String>,
}

/// The user notification listener for this app.
pub struct Listener {
    inner: UserNotificationListener,
}

impl Listener {
    /// The listener, or None when WinRT cannot provide one.
    pub fn open() -> Option<Self> {
        UserNotificationListener::Current()
            .ok()
            .map(|inner| Self { inner })
    }

    /// Whether Windows allows access now. An unreadable status counts as unknown.
    pub fn access(&self) -> Access {
        self.inner
            .GetAccessStatus()
            .map_or(Access::Unknown, to_access)
    }

    /// Asks Windows for access and returns the answer. Only called while Windows has not decided.
    pub fn request_access(&self) -> Access {
        wait(self.inner.RequestAccessAsync()).map_or(Access::Unknown, to_access)
    }

    /// The toasts Windows still lists, in the order it returns them. Reads nothing when access is
    /// not allowed, since Windows then returns an empty or failed list.
    pub fn toasts(&self) -> WinResult<Vec<Toast>> {
        let list = wait(self.inner.GetNotificationsAsync(NotificationKinds::Toast))?;
        let count = list.Size()?;
        let mut toasts = Vec::new();
        for index in 0..count {
            if let Ok(notification) = list.GetAt(index)
                && let Some(toast) = read(&notification)
            {
                toasts.push(toast);
            }
        }
        Ok(toasts)
    }
}

/// Blocks the poller until a WinRT operation completes. Only the poller calls it, on its own
/// thread, which has no message pump.
fn wait<O, T>(op: WinResult<O>) -> WinResult<T>
where
    O: IntoFuture<Output = WinResult<T>>,
{
    block_on(op?.into_future())
}

fn to_access(status: UserNotificationListenerAccessStatus) -> Access {
    if status == UserNotificationListenerAccessStatus::Allowed {
        Access::Allowed
    } else if status == UserNotificationListenerAccessStatus::Denied {
        Access::Denied
    } else {
        Access::Unspecified
    }
}

/// One toast's id, app and text. None when Windows gives no id for it.
fn read(notification: &UserNotification) -> Option<Toast> {
    let id = notification.Id().ok()?;
    let info = notification.AppInfo().ok()?;
    let app_name = info
        .DisplayInfo()
        .and_then(|display| display.DisplayName())
        .map(|name| name.to_string())
        .unwrap_or_default();
    let app_id = info
        .AppUserModelId()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let app_id = if app_id.is_empty() {
        app_name.clone()
    } else {
        app_id
    };
    let app_name = if app_name.is_empty() {
        app_id.clone()
    } else {
        app_name
    };

    let mut lines = Vec::new();
    if let Ok(visual) = notification.Notification().and_then(|n| n.Visual())
        && let Ok(bindings) = visual.Bindings()
    {
        for binding_index in 0..bindings.Size().unwrap_or(0) {
            let Ok(binding) = bindings.GetAt(binding_index) else {
                continue;
            };
            let Ok(texts) = binding.GetTextElements() else {
                continue;
            };
            for text_index in 0..texts.Size().unwrap_or(0) {
                if let Ok(text) = texts.GetAt(text_index).and_then(|element| element.Text()) {
                    lines.push(text.to_string());
                }
            }
        }
    }

    Some(Toast {
        id,
        app_id,
        app_name,
        lines,
    })
}

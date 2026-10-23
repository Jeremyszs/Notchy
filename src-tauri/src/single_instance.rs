//! One Notchy per user session. The first process holds a named mutex for its whole life, and a
//! second process (a manual start while the login entry already runs it, say) finds the name taken
//! and exits without showing anything. `Local\` keeps the name to this user's session.

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

/// The name every Notchy process races for.
pub const MUTEX_NAME: &str = "Local\\Notchy.SingleInstance";

/// Holds the mutex. Dropping it releases the name for the next process.
pub struct Guard(HANDLE);

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Claims `name`. Returns `None` when another process already holds it.
pub fn claim(name: &str) -> Option<Guard> {
    let name = HSTRING::from(name);
    let handle = unsafe { CreateMutexW(None, false, &name) }.ok()?;
    // Windows returns a handle to the existing mutex and sets this error when it was not ours.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    Some(Guard(handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name(tag: &str) -> String {
        format!("Local\\Notchy.Test.{}.{tag}", std::process::id())
    }

    #[test]
    fn second_claim_is_refused_until_the_first_is_dropped() {
        let name = unique_name("exclusive");
        let first = claim(&name).expect("first claim gets the name");
        assert!(claim(&name).is_none(), "second claim must be refused");
        drop(first);
        assert!(claim(&name).is_some(), "name is free again after drop");
    }

    #[test]
    fn different_names_do_not_conflict() {
        let a = claim(&unique_name("a")).expect("a");
        let b = claim(&unique_name("b")).expect("b");
        drop((a, b));
    }
}

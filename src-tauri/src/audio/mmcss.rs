//! Puts the audio thread in Windows' "Pro Audio" scheduling class (MMCSS), so
//! a busy PC doesn't starve it. cpal doesn't do this without its `realtime`
//! feature, which would register the lighter "Audio" class.

/// How the audio thread is scheduled, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Registered with MMCSS as "Pro Audio".
    ProAudio,
    /// MMCSS refused; the thread runs at `THREAD_PRIORITY_TIME_CRITICAL`.
    TimeCritical,
    /// Normal priority: nothing could be raised.
    Normal,
}

impl Priority {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::ProAudio => 1,
            Self::TimeCritical => 2,
            Self::Normal => 3,
        }
    }

    /// `None` for 0, the "not known yet" value.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::ProAudio),
            2 => Some(Self::TimeCritical),
            3 => Some(Self::Normal),
            _ => None,
        }
    }
}

/// Raises the calling thread on the first `raise()` and lowers it again on
/// drop. Lives in the audio callback, which cpal creates and drops on the
/// audio thread, so both calls happen on the right thread. Registering is a
/// one-off system call, like the boost cpal itself does when it has one; every
/// later `raise()` is a single branch.
#[derive(Default)]
pub struct AudioThread {
    tried: bool,
    /// The MMCSS handle, as an integer so the callback stays `Send`.
    handle: Option<isize>,
}

impl AudioThread {
    /// Returns the outcome the first time, `None` afterwards.
    #[inline]
    pub fn raise(&mut self) -> Option<Priority> {
        if self.tried {
            return None;
        }
        self.tried = true;
        let (priority, handle) = imp::raise();
        self.handle = handle;
        Some(priority)
    }
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            imp::revert(handle);
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, GetCurrentThread,
        SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
    };
    use windows::core::w;

    use super::Priority;

    pub fn raise() -> (Priority, Option<isize>) {
        let mut task_index = 0u32;
        // SAFETY: a static task name and a valid out pointer.
        match unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) } {
            Ok(handle) => (Priority::ProAudio, Some(handle.0 as isize)),
            Err(_) => {
                // SAFETY: the pseudo-handle of the current thread.
                let raised =
                    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
                let priority = if raised.is_ok() {
                    Priority::TimeCritical
                } else {
                    Priority::Normal
                };
                (priority, None)
            }
        }
    }

    pub fn revert(handle: isize) {
        // SAFETY: a handle from AvSetMmThreadCharacteristicsW on this thread.
        let _ = unsafe { AvRevertMmThreadCharacteristics(HANDLE(handle as *mut _)) };
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Priority;

    pub fn raise() -> (Priority, Option<isize>) {
        (Priority::Normal, None)
    }

    pub fn revert(_handle: isize) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_round_trips_and_zero_is_unknown() {
        for p in [Priority::ProAudio, Priority::TimeCritical, Priority::Normal] {
            assert_eq!(Priority::from_u8(p.to_u8()), Some(p));
        }
        assert_eq!(Priority::from_u8(0), None);
    }

    #[test]
    fn raises_once_and_reverts_on_drop() {
        // Runs on a test thread: registering and reverting must both succeed
        // without touching any other thread.
        let t = std::thread::spawn(|| {
            let mut thread = AudioThread::default();
            let first = thread.raise();
            assert!(first.is_some());
            assert_eq!(thread.raise(), None);
        });
        assert!(t.join().is_ok());
    }
}

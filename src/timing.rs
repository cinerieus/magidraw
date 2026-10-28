use std::thread;
use std::time::Duration;

pub fn scaled_ms(base_ms: u64, speed: f32, minimum_ms: u64) -> u64 {
    ((base_ms as f32 / speed.clamp(0.5, 20.0)).round() as u64).max(minimum_ms)
}

#[cfg(windows)]
mod windows_timer {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer,
        TIMER_ALL_ACCESS, WaitForSingleObject,
    };
    use windows::core::PCWSTR;

    pub struct PrecisionTimer(Option<HANDLE>);

    impl PrecisionTimer {
        pub fn new() -> Self {
            let handle = unsafe {
                CreateWaitableTimerExW(
                    None,
                    PCWSTR::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS.0,
                )
            }
            .ok();
            Self(handle)
        }

        pub fn sleep(&self, duration: std::time::Duration) {
            if duration.is_zero() {
                return;
            }
            if let Some(handle) = self.0 {
                let ticks = -((duration.as_nanos() / 100).max(1) as i64);
                if unsafe { SetWaitableTimer(handle, &ticks, 0, None, None, false) }.is_ok()
                    && unsafe { WaitForSingleObject(handle, INFINITE) } == WAIT_OBJECT_0
                {
                    return;
                }
            }
            super::fallback_sleep(duration);
        }
    }

    impl Drop for PrecisionTimer {
        fn drop(&mut self) {
            if let Some(handle) = self.0 {
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }
}

fn fallback_sleep(duration: Duration) {
    thread::sleep(duration);
}

#[cfg(windows)]
pub use windows_timer::PrecisionTimer;

#[cfg(not(windows))]
pub struct PrecisionTimer;

#[cfg(not(windows))]
impl PrecisionTimer {
    pub fn new() -> Self {
        Self
    }
    pub fn sleep(&self, duration: Duration) {
        fallback_sleep(duration);
    }
}

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread;
use std::time::Duration;

use crate::drawing::Plan;
use crate::timing::{PrecisionTimer, scaled_ms};

#[cfg(windows)]
mod win {
    use std::mem::size_of;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_MOVE_NOCOALESCE, MOUSEEVENTF_VIRTUALDESK,
        MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_ESCAPE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    pub fn key_down(key: VIRTUAL_KEY) -> bool {
        unsafe { GetAsyncKeyState(key.0 as i32) as u16 & 0x8000 != 0 }
    }

    pub fn cursor() -> Option<(i32, i32)> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point).ok()? };
        Some((point.x, point.y))
    }

    fn send(
        flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
        x: i32,
        y: i32,
    ) -> Result<(), String> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: x,
                    dy: y,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(format!(
                "SendInput sent {sent} events; check target privilege level"
            ))
        }
    }

    pub fn move_to(x: i32, y: i32) -> Result<(), String> {
        let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
        if width <= 1 || height <= 1 {
            return Err("Invalid virtual desktop dimensions".into());
        }
        let nx = ((x - left) as i64 * 65535 / (width - 1) as i64).clamp(0, 65535) as i32;
        let ny = ((y - top) as i64 * 65535 / (height - 1) as i64).clamp(0, 65535) as i32;
        send(
            MOUSEEVENTF_MOVE
                | MOUSEEVENTF_MOVE_NOCOALESCE
                | MOUSEEVENTF_ABSOLUTE
                | MOUSEEVENTF_VIRTUALDESK,
            nx,
            ny,
        )
    }

    pub fn down() -> Result<(), String> {
        send(MOUSEEVENTF_LEFTDOWN, 0, 0)
    }
    pub fn up() -> Result<(), String> {
        send(MOUSEEVENTF_LEFTUP, 0, 0)
    }
    pub fn escape() -> bool {
        key_down(VK_ESCAPE)
    }
    pub fn f6() -> bool {
        key_down(VIRTUAL_KEY(0x75))
    }
    pub fn f7() -> bool {
        key_down(VIRTUAL_KEY(0x76))
    }
    pub fn f8() -> bool {
        key_down(VIRTUAL_KEY(0x77))
    }
}

#[cfg(not(windows))]
mod win {
    pub fn cursor() -> Option<(i32, i32)> {
        None
    }
    pub fn escape() -> bool {
        false
    }
    pub fn f6() -> bool {
        false
    }
    pub fn f7() -> bool {
        false
    }
    pub fn f8() -> bool {
        false
    }
    pub fn move_to(_x: i32, _y: i32) -> Result<(), String> {
        Err("Drawing requires Windows".into())
    }
    pub fn down() -> Result<(), String> {
        Err("Drawing requires Windows".into())
    }
    pub fn up() -> Result<(), String> {
        Err("Drawing requires Windows".into())
    }
}

pub use win::{cursor, f6, f7, f8};

pub struct DrawJob {
    pub plan: Plan,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub delay_ms: u64,
    pub button_settle_ms: u64,
    pub pen_lift_ms: u64,
    pub speed: f32,
    pub countdown: u64,
    pub cancel: Arc<AtomicBool>,
    pub updates: Sender<String>,
}

struct MouseGuard(bool);

impl Drop for MouseGuard {
    fn drop(&mut self) {
        if self.0 {
            let _ = win::up();
        }
    }
}

fn stopped(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed) || win::escape()
}

pub fn run(job: DrawJob) {
    let result = draw(&job);
    let message = match result {
        Ok(()) => "Done".to_owned(),
        Err(error) => error,
    };
    let _ = job.updates.send(message);
}

fn draw(job: &DrawJob) -> Result<(), String> {
    for remaining in (1..=job.countdown).rev() {
        if stopped(&job.cancel) {
            return Err("Stopped".into());
        }
        let _ = job.updates.send(format!(
            "Drawing starts in {remaining}s. Focus the whiteboard."
        ));
        for _ in 0..20 {
            if stopped(&job.cancel) {
                return Err("Stopped".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    let offset_x = job.left + (job.width as i32 - job.plan.image_width as i32) / 2;
    let offset_y = job.top + (job.height as i32 - job.plan.image_height as i32) / 2;
    let total = job.plan.strokes.len();
    let timer = PrecisionTimer::new();
    let move_ms = scaled_ms(job.delay_ms, job.speed, 0);
    let button_ms = scaled_ms(job.button_settle_ms, job.speed, 1);
    let lift_ms = scaled_ms(job.pen_lift_ms, job.speed, 1);
    win::up()?;
    timer.sleep(Duration::from_millis(lift_ms));
    for (index, stroke) in job.plan.strokes.iter().enumerate() {
        if stopped(&job.cancel) {
            return Err("Stopped".into());
        }
        let Some(&(first_x, first_y)) = stroke.points.first() else {
            continue;
        };
        win::move_to(offset_x + first_x, offset_y + first_y)?;
        timer.sleep(Duration::from_millis(button_ms));
        win::down()?;
        let mut guard = MouseGuard(true);
        timer.sleep(Duration::from_millis(button_ms));
        for segment in stroke.points.windows(2) {
            let (from_x, from_y) = segment[0];
            let (to_x, to_y) = segment[1];
            let dx = to_x - from_x;
            let dy = to_y - from_y;
            let distance = dx.abs().max(dy.abs()) as u32;
            let steps = distance.div_ceil(job.plan.point_step).max(1) as i32;
            for part in 1..=steps {
                if stopped(&job.cancel) {
                    return Err("Stopped".into());
                }
                let x = from_x + dx * part / steps;
                let y = from_y + dy * part / steps;
                win::move_to(offset_x + x, offset_y + y)?;
                timer.sleep(Duration::from_millis(move_ms));
            }
        }
        timer.sleep(Duration::from_millis(button_ms));
        win::up()?;
        guard.0 = false;
        timer.sleep(Duration::from_millis(lift_ms));
        if index % 100 == 0 {
            let _ = job
                .updates
                .send(format!("Drawing {}/{total} strokes", index + 1));
        }
    }
    Ok(())
}

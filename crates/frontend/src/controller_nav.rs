//! Using Flux itself with a controller: reads Xbox controllers through XInput (PlayStation
//! controllers too, when Flux shows them to games as Xbox controllers) and turns them into
//! menu moves. `ui.rs` polls it while Flux's window is in front, so a game being played never
//! moves Flux's menus.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use windows::Win32::UI::Input::XboxController::{
    XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK, XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT,
    XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X,
    XINPUT_GAMEPAD_Y, XINPUT_STATE, XInputGetState,
};

/// What a controller press means in Flux's menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A / Cross
    Confirm,
    /// B / Circle
    Back,
    /// X / Square
    X,
    /// Y / Triangle
    Y,
    /// LB / L1
    PrevTab,
    /// RB / R1
    NextTab,
    /// LT / L2
    PrevPage,
    /// RT / R2
    NextPage,
    /// View / Share
    View,
    /// Menu / Options
    Start,
}

static CONNECTED: AtomicBool = AtomicBool::new(false);

/// Is a controller plugged in (seen in the last poll)?
pub fn connected() -> bool {
    CONNECTED.load(Ordering::Relaxed)
}

const STICK: i16 = 16000;
const RIGHT_DEADZONE: f32 = 0.25;
const FIRST_REPEAT: Duration = Duration::from_millis(380);
const REPEAT: Duration = Duration::from_millis(110);

/// One press of the controller, as bits (so held buttons only count once).
const BUTTONS: [(u16, Nav); 8] = [
    (XINPUT_GAMEPAD_A.0, Nav::Confirm),
    (XINPUT_GAMEPAD_B.0, Nav::Back),
    (XINPUT_GAMEPAD_X.0, Nav::X),
    (XINPUT_GAMEPAD_Y.0, Nav::Y),
    (XINPUT_GAMEPAD_LEFT_SHOULDER.0, Nav::PrevTab),
    (XINPUT_GAMEPAD_RIGHT_SHOULDER.0, Nav::NextTab),
    (XINPUT_GAMEPAD_BACK.0, Nav::View),
    (XINPUT_GAMEPAD_START.0, Nav::Start),
];

#[derive(Default)]
pub struct Poller {
    buttons: u16,
    triggers: (bool, bool),
    direction: Option<Nav>,
    next_repeat: Option<Instant>,
    /// Slots with nothing plugged in are only checked now and then (an empty slot is slow to ask).
    slots: [Option<Instant>; 4],
    primed: bool,
}

/// What the controller did since the last poll.
#[derive(Default)]
pub struct Polled {
    pub presses: Vec<Nav>,
    /// Right stick, -1..1 (up is positive), for scrolling.
    pub scroll: f32,
}

impl Poller {
    pub fn poll(&mut self) -> Polled {
        let mut buttons = 0u16;
        let (mut lx, mut ly, mut ry) = (0i16, 0i16, 0i16);
        let (mut lt, mut rt) = (0u8, 0u8);
        let mut any = false;
        let now = Instant::now();
        for user in 0..4u32 {
            if self.slots[user as usize].is_some_and(|retry| now < retry) {
                continue;
            }
            let mut state = XINPUT_STATE::default();
            if unsafe { XInputGetState(user, &mut state) } != 0 {
                self.slots[user as usize] = Some(now + Duration::from_secs(2));
                continue;
            }
            self.slots[user as usize] = None;
            any = true;
            let pad = state.Gamepad;
            buttons |= pad.wButtons.0;
            lt = lt.max(pad.bLeftTrigger);
            rt = rt.max(pad.bRightTrigger);
            if pad.sThumbLX.unsigned_abs() > lx.unsigned_abs() {
                lx = pad.sThumbLX;
            }
            if pad.sThumbLY.unsigned_abs() > ly.unsigned_abs() {
                ly = pad.sThumbLY;
            }
            if pad.sThumbRY.unsigned_abs() > ry.unsigned_abs() {
                ry = pad.sThumbRY;
            }
        }
        CONNECTED.store(any, Ordering::Relaxed);
        let mut out = Polled::default();
        let triggers = (lt > 128, rt > 128);

        // The first look only learns what's already held (e.g. A still down from launching Flux).
        if !self.primed {
            self.primed = true;
            self.buttons = buttons;
            self.triggers = triggers;
            return out;
        }

        let pressed = buttons & !self.buttons;
        self.buttons = buttons;
        out.presses.extend(BUTTONS.iter().filter(|(bit, _)| pressed & bit != 0).map(|(_, nav)| *nav));
        if triggers.0 && !self.triggers.0 {
            out.presses.push(Nav::PrevPage);
        }
        if triggers.1 && !self.triggers.1 {
            out.presses.push(Nav::NextPage);
        }
        self.triggers = triggers;

        // D-pad or left stick, repeating while held.
        let direction = if buttons & XINPUT_GAMEPAD_DPAD_UP.0 != 0 || ly > STICK {
            Some(Nav::Up)
        } else if buttons & XINPUT_GAMEPAD_DPAD_DOWN.0 != 0 || ly < -STICK {
            Some(Nav::Down)
        } else if buttons & XINPUT_GAMEPAD_DPAD_LEFT.0 != 0 || lx < -STICK {
            Some(Nav::Left)
        } else if buttons & XINPUT_GAMEPAD_DPAD_RIGHT.0 != 0 || lx > STICK {
            Some(Nav::Right)
        } else {
            None
        };
        if direction != self.direction {
            self.direction = direction;
            if let Some(d) = direction {
                out.presses.push(d);
                self.next_repeat = Some(now + FIRST_REPEAT);
            }
        } else if let (Some(d), Some(at)) = (direction, self.next_repeat) && now >= at {
            out.presses.push(d);
            self.next_repeat = Some(now + REPEAT);
        }

        let ry = ry as f32 / 32767.;
        if ry.abs() > RIGHT_DEADZONE {
            out.scroll = (ry.abs() - RIGHT_DEADZONE) / (1. - RIGHT_DEADZONE) * ry.signum();
        }
        out
    }
}

//! Button mapping: which Xbox control each PlayStation control presses, plus stick options. The
//! player edits it on the Controllers page; every controller uses it from the next report on.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::vigem::XReport;

/// A PlayStation controller's buttons as read from the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsButton {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Create,
    Options,
    Ps,
    Touchpad,
    Mute,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl PsButton {
    pub const ALL: [PsButton; 19] = [
        PsButton::Cross, PsButton::Circle, PsButton::Square, PsButton::Triangle,
        PsButton::L1, PsButton::R1, PsButton::L2, PsButton::R2, PsButton::L3, PsButton::R3,
        PsButton::Create, PsButton::Options, PsButton::Ps, PsButton::Touchpad, PsButton::Mute,
        PsButton::DpadUp, PsButton::DpadDown, PsButton::DpadLeft, PsButton::DpadRight,
    ];

    pub fn bit(self) -> u32 {
        1 << self as u32
    }

    pub fn name(self) -> &'static str {
        match self {
            PsButton::Cross => "Cross ✕",
            PsButton::Circle => "Circle ○",
            PsButton::Square => "Square □",
            PsButton::Triangle => "Triangle △",
            PsButton::L1 => "L1",
            PsButton::R1 => "R1",
            PsButton::L2 => "L2",
            PsButton::R2 => "R2",
            PsButton::L3 => "L3 (press left stick)",
            PsButton::R3 => "R3 (press right stick)",
            PsButton::Create => "Create / Share",
            PsButton::Options => "Options",
            PsButton::Ps => "PS button",
            PsButton::Touchpad => "Touchpad click",
            PsButton::Mute => "Mute (PS5 only)",
            PsButton::DpadUp => "D-pad up",
            PsButton::DpadDown => "D-pad down",
            PsButton::DpadLeft => "D-pad left",
            PsButton::DpadRight => "D-pad right",
        }
    }

    /// What it presses out of the box: the button in the same place on an Xbox controller.
    pub fn default_target(self) -> XboxControl {
        match self {
            PsButton::Cross => XboxControl::A,
            PsButton::Circle => XboxControl::B,
            PsButton::Square => XboxControl::X,
            PsButton::Triangle => XboxControl::Y,
            PsButton::L1 => XboxControl::LeftBumper,
            PsButton::R1 => XboxControl::RightBumper,
            PsButton::L2 => XboxControl::LeftTrigger,
            PsButton::R2 => XboxControl::RightTrigger,
            PsButton::L3 => XboxControl::LeftStickPress,
            PsButton::R3 => XboxControl::RightStickPress,
            PsButton::Create => XboxControl::Back,
            PsButton::Options => XboxControl::Start,
            PsButton::Ps => XboxControl::Guide,
            PsButton::Touchpad => XboxControl::Back,
            PsButton::Mute => XboxControl::Nothing,
            PsButton::DpadUp => XboxControl::DpadUp,
            PsButton::DpadDown => XboxControl::DpadDown,
            PsButton::DpadLeft => XboxControl::DpadLeft,
            PsButton::DpadRight => XboxControl::DpadRight,
        }
    }
}

/// What a PlayStation button can press on the virtual Xbox controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XboxControl {
    A,
    B,
    X,
    Y,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    LeftStickPress,
    RightStickPress,
    Back,
    Start,
    Guide,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    Nothing,
}

impl XboxControl {
    pub const ALL: [XboxControl; 18] = [
        XboxControl::A, XboxControl::B, XboxControl::X, XboxControl::Y,
        XboxControl::LeftBumper, XboxControl::RightBumper, XboxControl::LeftTrigger, XboxControl::RightTrigger,
        XboxControl::LeftStickPress, XboxControl::RightStickPress, XboxControl::Back, XboxControl::Start, XboxControl::Guide,
        XboxControl::DpadUp, XboxControl::DpadDown, XboxControl::DpadLeft, XboxControl::DpadRight, XboxControl::Nothing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            XboxControl::A => "A",
            XboxControl::B => "B",
            XboxControl::X => "X",
            XboxControl::Y => "Y",
            XboxControl::LeftBumper => "LB",
            XboxControl::RightBumper => "RB",
            XboxControl::LeftTrigger => "LT",
            XboxControl::RightTrigger => "RT",
            XboxControl::LeftStickPress => "Left stick press",
            XboxControl::RightStickPress => "Right stick press",
            XboxControl::Back => "View / Back",
            XboxControl::Start => "Menu / Start",
            XboxControl::Guide => "Xbox button",
            XboxControl::DpadUp => "D-pad up",
            XboxControl::DpadDown => "D-pad down",
            XboxControl::DpadLeft => "D-pad left",
            XboxControl::DpadRight => "D-pad right",
            XboxControl::Nothing => "Nothing",
        }
    }

    /// The XInput button bit (triggers and Nothing have none).
    fn button_bit(self) -> u16 {
        match self {
            XboxControl::DpadUp => 0x0001,
            XboxControl::DpadDown => 0x0002,
            XboxControl::DpadLeft => 0x0004,
            XboxControl::DpadRight => 0x0008,
            XboxControl::Start => 0x0010,
            XboxControl::Back => 0x0020,
            XboxControl::LeftStickPress => 0x0040,
            XboxControl::RightStickPress => 0x0080,
            XboxControl::LeftBumper => 0x0100,
            XboxControl::RightBumper => 0x0200,
            XboxControl::Guide => 0x0400,
            XboxControl::A => 0x1000,
            XboxControl::B => 0x2000,
            XboxControl::X => 0x4000,
            XboxControl::Y => 0x8000,
            XboxControl::LeftTrigger | XboxControl::RightTrigger | XboxControl::Nothing => 0,
        }
    }
}

/// Everything read from a PlayStation controller, before mapping. Sticks are raw (0-255, 128 is
/// centre, up is 0), triggers 0-255.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PsState {
    /// `PsButton::bit()`s of the buttons held.
    pub buttons: u32,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    pub l2: u8,
    pub r2: u8,
}

impl PsState {
    pub fn held(&self, button: PsButton) -> bool {
        self.buttons & button.bit() != 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    /// What each PlayStation button presses; buttons left out press their default.
    pub buttons: BTreeMap<PsButton, XboxControl>,
    /// Left stick acts as the right one and the other way round.
    pub swap_sticks: bool,
    pub invert_left_y: bool,
    pub invert_right_y: bool,
    /// Stick dead zones, percent of the full tilt (0-40).
    pub left_deadzone: u8,
    pub right_deadzone: u8,
    /// L2/R2 pulled less than this percent count as released (0-60).
    pub trigger_deadzone: u8,
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping { buttons: BTreeMap::new(), swap_sticks: false, invert_left_y: false, invert_right_y: false, left_deadzone: 5, right_deadzone: 5, trigger_deadzone: 0 }
    }
}

/// Ready-made mappings for the Controllers page.
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub apply: fn(&mut Mapping),
}

pub const PRESETS: &[Preset] = &[
    Preset { name: "Xbox layout", description: "Every button where the Xbox one is (the default)", apply: |m| *m = Mapping::default() },
    Preset {
        name: "Nintendo layout",
        description: "Cross is B, Circle is A, Square is Y, Triangle is X",
        apply: |m| {
            m.buttons.insert(PsButton::Cross, XboxControl::B);
            m.buttons.insert(PsButton::Circle, XboxControl::A);
            m.buttons.insert(PsButton::Square, XboxControl::Y);
            m.buttons.insert(PsButton::Triangle, XboxControl::X);
        },
    },
    Preset {
        name: "Bumpers ⇄ triggers",
        description: "L1/R1 pull the triggers, L2/R2 press the bumpers",
        apply: |m| {
            m.buttons.insert(PsButton::L1, XboxControl::LeftTrigger);
            m.buttons.insert(PsButton::R1, XboxControl::RightTrigger);
            m.buttons.insert(PsButton::L2, XboxControl::LeftBumper);
            m.buttons.insert(PsButton::R2, XboxControl::RightBumper);
        },
    },
    Preset { name: "Southpaw", description: "Swaps the sticks", apply: |m| m.swap_sticks = !m.swap_sticks },
];

fn axis(v: u8) -> i16 {
    ((v as i32 - 128) * 32767 / 127).clamp(-32768, 32767) as i16
}

/// One stick (raw PlayStation values) as Xbox values with a radial dead zone; `y` comes out with up
/// positive, as on Xbox, unless inverted.
fn stick(x: u8, y: u8, deadzone: u8, invert_y: bool) -> (i16, i16) {
    let (fx, fy) = (axis(x) as f32 / 32767.0, -(axis(y) as f32) / 32767.0);
    let fy = if invert_y { -fy } else { fy };
    let magnitude = (fx * fx + fy * fy).sqrt();
    let dz = deadzone.min(40) as f32 / 100.0;
    if magnitude <= dz || magnitude == 0.0 {
        return (0, 0);
    }
    // Rescale so movement starts from zero just outside the dead zone.
    let scale = if dz > 0.0 { ((magnitude - dz) / (1.0 - dz)).min(1.0) / magnitude } else { 1.0 };
    let out = |v: f32| (v * scale * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
    (out(fx), out(fy))
}

impl Mapping {
    pub fn target(&self, button: PsButton) -> XboxControl {
        self.buttons.get(&button).copied().unwrap_or(button.default_target())
    }

    pub fn set(&mut self, button: PsButton, control: XboxControl) {
        if control == button.default_target() {
            self.buttons.remove(&button);
        } else {
            self.buttons.insert(button, control);
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Mapping::default()
    }

    pub fn apply(&self, s: &PsState) -> XReport {
        let mut report = XReport::default();
        let threshold = (self.trigger_deadzone.min(60) as u32 * 255 / 100) as u8;
        let trigger = |v: u8| if v <= threshold { 0 } else { v };
        for button in PsButton::ALL {
            // L2/R2 are analog: they press their target when pulled past the dead zone.
            let (held, analog) = match button {
                PsButton::L2 => (trigger(s.l2) > 0, trigger(s.l2)),
                PsButton::R2 => (trigger(s.r2) > 0, trigger(s.r2)),
                b => (s.held(b), if s.held(b) { 255 } else { 0 }),
            };
            if !held {
                continue;
            }
            match self.target(button) {
                XboxControl::LeftTrigger => report.left_trigger = report.left_trigger.max(analog),
                XboxControl::RightTrigger => report.right_trigger = report.right_trigger.max(analog),
                control => report.buttons |= control.button_bit(),
            }
        }
        let (left, right) = ((s.lx, s.ly), (s.rx, s.ry));
        let (left, right) = if self.swap_sticks { (right, left) } else { (left, right) };
        (report.lx, report.ly) = stick(left.0, left.1, self.left_deadzone, self.invert_left_y);
        (report.rx, report.ry) = stick(right.0, right.1, self.right_deadzone, self.invert_right_y);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centred() -> PsState {
        PsState { lx: 128, ly: 128, rx: 128, ry: 128, ..Default::default() }
    }

    #[test]
    fn default_mapping_is_the_xbox_layout() {
        let m = Mapping { left_deadzone: 0, right_deadzone: 0, ..Mapping::default() };
        let s = PsState { buttons: PsButton::Cross.bit() | PsButton::Ps.bit() | PsButton::DpadUp.bit(), l2: 200, lx: 255, ly: 0, ..centred() };
        let r = m.apply(&s);
        assert_eq!(r.buttons, 0x1000 | 0x0400 | 0x0001);
        assert_eq!(r.left_trigger, 200);
        assert_eq!((r.lx, r.ly), (32767, 32767), "stick up is positive on Xbox");
        let full_left = m.apply(&PsState { lx: 0, ..centred() });
        assert_eq!(full_left.lx, -32768);
    }

    #[test]
    fn remaps_and_presets() {
        let mut m = Mapping::default();
        (PRESETS[1].apply)(&mut m);
        let r = m.apply(&PsState { buttons: PsButton::Cross.bit(), ..centred() });
        assert_eq!(r.buttons, 0x2000, "Nintendo layout: Cross is B");

        let mut m = Mapping::default();
        (PRESETS[2].apply)(&mut m);
        let r = m.apply(&PsState { buttons: PsButton::R1.bit(), r2: 255, ..centred() });
        assert_eq!(r.right_trigger, 255, "R1 pulls RT fully");
        assert_eq!(r.buttons, 0x0200, "R2 presses RB");

        m.set(PsButton::R1, XboxControl::RightTrigger);
        m.set(PsButton::Mute, XboxControl::Nothing);
        assert!(!m.buttons.contains_key(&PsButton::Mute), "setting a default removes the override");
        m.set(PsButton::Touchpad, XboxControl::Nothing);
        assert_eq!(m.apply(&PsState { buttons: PsButton::Touchpad.bit(), ..centred() }).buttons, 0);
    }

    #[test]
    fn sticks_deadzones_and_swaps() {
        let m = Mapping { left_deadzone: 10, ..Mapping::default() };
        assert_eq!(m.apply(&PsState { lx: 135, ..centred() }).lx, 0, "small drift is ignored");
        assert_eq!(m.apply(&PsState { lx: 255, ..centred() }).lx, 32767, "full tilt still reaches the edge");
        let swapped = Mapping { swap_sticks: true, ..Mapping::default() };
        let r = swapped.apply(&PsState { lx: 255, ..centred() });
        assert_eq!((r.lx, r.rx), (0, 32767));
        let inverted = Mapping { invert_right_y: true, ..Mapping::default() };
        assert_eq!(inverted.apply(&PsState { ry: 0, ..centred() }).ry, -32767, "inverted: stick up reads as down");
        let m = Mapping { trigger_deadzone: 20, ..Mapping::default() };
        assert_eq!(m.apply(&PsState { l2: 40, ..centred() }).left_trigger, 0);
        assert_eq!(m.apply(&PsState { l2: 200, ..centred() }).left_trigger, 200);
    }
}

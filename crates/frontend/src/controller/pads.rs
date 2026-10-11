//! PlayStation controllers over USB and Bluetooth: decoding input reports into an Xbox 360 state and
//! building rumble/lightbar output reports. Report layouts follow Linux's hid-playstation driver.

use super::mapping::{PsButton, PsState};

pub const SONY: u16 = 0x054C;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    DualSense,
    DualSenseEdge,
    DualShock4,
}

impl Model {
    pub fn from_ids(vendor: u16, product: u16) -> Option<Model> {
        if vendor != SONY {
            return None;
        }
        match product {
            0x0CE6 => Some(Model::DualSense),
            0x0DF2 => Some(Model::DualSenseEdge),
            0x05C4 | 0x09CC | 0x0BA0 => Some(Model::DualShock4),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::DualSense => "DualSense (PS5)",
            Model::DualSenseEdge => "DualSense Edge (PS5)",
            Model::DualShock4 => "DualShock 4 (PS4)",
        }
    }

    fn is_dualsense(self) -> bool {
        matches!(self, Model::DualSense | Model::DualSenseEdge)
    }
}

/// What one input report says.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Input {
    /// Buttons, sticks and triggers as the controller reports them (mapped to Xbox in `mapping`).
    pub state: PsState,
    /// Battery percentage, when the report carries it.
    pub battery: Option<u8>,
    pub charging: bool,
    /// The report came over Bluetooth.
    pub bluetooth: bool,
}

fn dpad(hat: u8) -> u32 {
    let (up, down, left, right) = (PsButton::DpadUp.bit(), PsButton::DpadDown.bit(), PsButton::DpadLeft.bit(), PsButton::DpadRight.bit());
    match hat & 0x0F {
        0 => up,
        1 => up | right,
        2 => right,
        3 => down | right,
        4 => down,
        5 => down | left,
        6 => left,
        7 => up | left,
        _ => 0,
    }
}

/// Face buttons + d-pad byte, shoulder/stick byte, and system byte (PS, touchpad, mute) are laid
/// out the same way on DualSense and DualShock 4.
fn buttons(b0: u8, b1: u8, b2: u8) -> u32 {
    let mut out = dpad(b0);
    let bit = |byte: u8, n: u8, button: PsButton| if byte & (1 << n) != 0 { button.bit() } else { 0 };
    out |= bit(b0, 4, PsButton::Square) | bit(b0, 5, PsButton::Cross) | bit(b0, 6, PsButton::Circle) | bit(b0, 7, PsButton::Triangle);
    out |= bit(b1, 0, PsButton::L1) | bit(b1, 1, PsButton::R1) | bit(b1, 2, PsButton::L2) | bit(b1, 3, PsButton::R2);
    out |= bit(b1, 4, PsButton::Create) | bit(b1, 5, PsButton::Options);
    out |= bit(b1, 6, PsButton::L3) | bit(b1, 7, PsButton::R3);
    out |= bit(b2, 0, PsButton::Ps) | bit(b2, 1, PsButton::Touchpad) | bit(b2, 2, PsButton::Mute);
    out
}

/// Sticks (lx, ly, rx, ry), triggers and the three button bytes.
fn state(sticks: &[u8], l2: u8, r2: u8, b: &[u8]) -> PsState {
    PsState { buttons: buttons(b[0], b[1], b[2]), lx: sticks[0], ly: sticks[1], rx: sticks[2], ry: sticks[3], l2, r2 }
}

/// Decode one input report (`data[0]` is the report id). None for reports that aren't input.
pub fn parse(model: Model, data: &[u8]) -> Option<Input> {
    let id = *data.first()?;
    if model.is_dualsense() {
        // USB 0x01 (64 bytes) and Bluetooth 0x31 share the full layout, one byte apart; Bluetooth
        // starts with a short 0x01 report until the controller is switched to full reports.
        let (o, bluetooth) = match (id, data.len()) {
            (0x01, n) if n >= 64 => (1, false),
            (0x31, n) if n >= 64 => (2, true),
            (0x01, n) if n >= 10 => {
                let state = state(&data[1..5], data[8], data[9], &data[5..8]);
                return Some(Input { state, battery: None, charging: false, bluetooth: true });
            },
            _ => return None,
        };
        let d = &data[o..];
        let state = state(&d[0..4], d[4], d[5], &d[7..10]);
        // Byte 52 of the common block: level 0-10 in the low nibble, charging status in the high one.
        let status = d.get(52).copied().unwrap_or(0);
        let battery = Some(((status & 0x0F) as u32 * 10 + 5).min(100) as u8);
        let charging = status >> 4 == 1;
        return Some(Input { state, battery, charging, bluetooth });
    }

    // DualShock 4: USB 0x01, Bluetooth 0x11 (two bytes later); short Bluetooth 0x01 reports are
    // laid out like USB.
    let (o, bluetooth) = match (id, data.len()) {
        (0x01, n) if n >= 64 => (1, false),
        (0x11, n) if n >= 64 => (3, true),
        (0x01, n) if n >= 10 => (1, true),
        _ => return None,
    };
    let d = &data[o..];
    let mut state = state(&d[0..4], d[7], d[8], &d[4..7]);
    state.buttons &= !PsButton::Mute.bit(); // that bit is part of a frame counter on the DualShock 4
    let battery = d.get(29).map(|s| {
        let level = (s & 0x0F) as u32;
        (if s & 0x10 != 0 { level * 100 / 11 } else { level * 10 + 5 }).min(100) as u8
    });
    let charging = d.get(29).is_some_and(|s| s & 0x10 != 0);
    Some(Input { state, battery, charging, bluetooth })
}

/// CRC-32 (IEEE) of the Bluetooth output seed byte 0xA2 followed by `data`.
fn bt_crc(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in std::iter::once(&0xA2u8).chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Rumble (large = left/low motor, small = right/high motor) and lightbar colour.
pub fn output(model: Model, bluetooth: bool, seq: u8, large: u8, small: u8, rgb: [u8; 3]) -> Option<Vec<u8>> {
    if model.is_dualsense() {
        // The 47-byte "common" block (hid-playstation's dualsense_output_report_common).
        let mut common = [0u8; 47];
        common[0] = 0x01 | 0x02; // valid_flag0: compatible vibration, haptics select
        common[1] = 0x04; // valid_flag1: lightbar control
        common[2] = small; // motor_right
        common[3] = large; // motor_left
        common[38] = 0x04; // valid_flag2: compatible vibration v2 (newer firmware)
        common[44] = rgb[0];
        common[45] = rgb[1];
        common[46] = rgb[2];
        if !bluetooth {
            let mut report = vec![0x02];
            report.extend_from_slice(&common);
            return Some(report);
        }
        let mut report = vec![0u8; 78];
        report[0] = 0x31;
        report[1] = (seq & 0x0F) << 4;
        report[2] = 0x10; // tag
        report[3..50].copy_from_slice(&common);
        let crc = bt_crc(&report[..74]);
        report[74..78].copy_from_slice(&crc.to_le_bytes());
        return Some(report);
    }
    if !bluetooth {
        let mut report = vec![0u8; 32];
        report[0] = 0x05;
        report[1] = 0x01 | 0x02; // rumble + lightbar
        report[4] = small;
        report[5] = large;
        report[6..9].copy_from_slice(&rgb);
        return Some(report);
    }
    let mut report = vec![0u8; 78];
    report[0] = 0x11;
    report[1] = 0xC0; // HID + CRC
    report[3] = 0x01 | 0x02;
    report[6] = small;
    report[7] = large;
    report[8..11].copy_from_slice(&rgb);
    let crc = bt_crc(&report[..74]);
    report[74..78].copy_from_slice(&crc.to_le_bytes());
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{mapping::Mapping, vigem::XReport};

    const A: u16 = 0x1000;
    const X: u16 = 0x4000;
    const Y: u16 = 0x8000;
    const DPAD_DOWN: u16 = 0x0002;
    const DPAD_RIGHT: u16 = 0x0008;
    const START: u16 = 0x0010;
    const LEFT_SHOULDER: u16 = 0x0100;
    const RIGHT_SHOULDER: u16 = 0x0200;
    const GUIDE: u16 = 0x0400;

    /// The Xbox report with the default mapping and no dead zones.
    fn xbox(input: &Input) -> XReport {
        Mapping { left_deadzone: 0, right_deadzone: 0, ..Mapping::default() }.apply(&input.state)
    }

    fn dualsense_usb(f: impl FnOnce(&mut [u8])) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[128, 128, 128, 128]);
        r[8] = 0x08; // d-pad released
        f(&mut r);
        r
    }

    #[test]
    fn dualsense_usb_maps_to_xbox() {
        let idle = parse(Model::DualSense, &dualsense_usb(|_| {})).unwrap();
        assert_eq!(xbox(&idle).buttons, 0);
        assert_eq!((xbox(&idle).lx, xbox(&idle).ly), (0, 0));
        assert!(!idle.bluetooth);

        let r = parse(Model::DualSense, &dualsense_usb(|r| {
            r[1] = 255; // left stick right
            r[2] = 0; // left stick up
            r[5] = 200; // L2
            r[8] = 0x20 | 0x02; // cross + d-pad right
            r[9] = 0x01 | 0x20; // L1 + options
            r[10] = 0x01; // PS
            r[53] = 0x1A; // status byte (common offset 52): charging, level 10
        })).unwrap();
        assert_eq!(xbox(&r).lx, 32767);
        assert_eq!(xbox(&r).ly, 32767, "stick up is positive on Xbox");
        assert_eq!(xbox(&r).left_trigger, 200);
        assert_eq!(xbox(&r).buttons, A | DPAD_RIGHT | LEFT_SHOULDER | START | GUIDE);
        assert_eq!((r.battery, r.charging), (Some(100), true));
    }

    #[test]
    fn dualsense_bluetooth_is_one_byte_later() {
        let usb = dualsense_usb(|r| { r[8] = 0x80 | 0x08; r[6] = 77; });
        let mut bt = vec![0x31u8, 0x00];
        bt.extend_from_slice(&usb[1..]);
        bt.resize(78, 0);
        let a = parse(Model::DualSense, &usb).unwrap();
        let b = parse(Model::DualSense, &bt).unwrap();
        assert_eq!(a.state, b.state);
        assert_eq!(xbox(&b).buttons, Y);
        assert_eq!(xbox(&b).right_trigger, 77);
        assert!(b.bluetooth);
    }

    #[test]
    fn dualshock4_usb() {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[0, 128, 128, 255]);
        r[5] = 0x10 | 0x04; // square + d-pad down
        r[6] = 0x02; // R1
        r[9] = 255; // R2
        let i = parse(Model::DualShock4, &r).unwrap();
        assert_eq!(xbox(&i).lx, -32768, "full left is the bottom of the Xbox range");
        assert_eq!(xbox(&i).ry, -32767);
        assert_eq!(xbox(&i).buttons, X | DPAD_DOWN | RIGHT_SHOULDER);
        assert_eq!(xbox(&i).right_trigger, 255);
    }

    #[test]
    fn output_reports() {
        let usb = output(Model::DualSense, false, 0, 200, 50, [1, 2, 3]).unwrap();
        assert_eq!(usb.len(), 48);
        assert_eq!((usb[0], usb[3], usb[4]), (0x02, 50, 200));
        assert_eq!(&usb[45..48], &[1, 2, 3]);
        let bt = output(Model::DualSense, true, 3, 200, 50, [1, 2, 3]).unwrap();
        assert_eq!(bt.len(), 78);
        assert_eq!(u32::from_le_bytes(bt[74..78].try_into().unwrap()), bt_crc(&bt[..74]));
        // Known CRC-32 check value for "123456789" without the seed byte is 0xCBF43926.
        let mut plain = !0u32;
        for &b in b"123456789" {
            plain ^= b as u32;
            for _ in 0..8 {
                plain = if plain & 1 != 0 { (plain >> 1) ^ 0xEDB8_8320 } else { plain >> 1 };
            }
        }
        assert_eq!(!plain, 0xCBF4_3926);
    }
}

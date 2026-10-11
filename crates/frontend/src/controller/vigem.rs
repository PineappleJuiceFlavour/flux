//! A small client for the ViGEmBus driver (virtual game controllers), speaking its I/O-control
//! protocol directly. Layouts and codes follow ViGEmClient's `BusShared.h` (MIT, Nefarius).

use std::ffi::c_void;

use windows::{
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
            SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
        },
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_FLAG_WRITE_THROUGH, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::{IO::{DeviceIoControl, GetOverlappedResult, OVERLAPPED}, Threading::CreateEventW},
    },
    core::{GUID, PCWSTR},
};

pub const BUS_INTERFACE: GUID = GUID::from_values(0x96E42B22, 0xF5E9, 0x42F8, [0xB0, 0x43, 0xED, 0x0F, 0x93, 0x2F, 0x01, 0x4F]);

/// CTL_CODE(FILE_DEVICE_BUS_EXTENDER, function, METHOD_BUFFERED, access)
const fn ctl(function: u32, access: u32) -> u32 {
    (0x2A << 16) | (access << 14) | (function << 2)
}
const WRITE: u32 = 2; // FILE_WRITE_DATA
const READ_WRITE: u32 = 3; // FILE_READ_DATA | FILE_WRITE_DATA
pub const IOCTL_PLUGIN_TARGET: u32 = ctl(0x801, WRITE);
pub const IOCTL_UNPLUG_TARGET: u32 = ctl(0x802, WRITE);
pub const IOCTL_CHECK_VERSION: u32 = ctl(0x803, WRITE);
pub const IOCTL_WAIT_DEVICE_READY: u32 = ctl(0x804, WRITE);
pub const IOCTL_XUSB_REQUEST_NOTIFICATION: u32 = ctl(0xA01, READ_WRITE);
pub const IOCTL_XUSB_SUBMIT_REPORT: u32 = ctl(0xA02, WRITE);
const COMMON_VERSION: u32 = 1;
const MAX_TARGETS: u32 = 16;

/// Device paths of every present interface of a device-interface class.
pub fn interface_paths(class: &GUID) -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(Some(class), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) else { return paths };
        let mut index = 0;
        loop {
            let mut data = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(set, None, class, index, &mut data).is_err() {
                break;
            }
            index += 1;
            let mut required = 0u32;
            _ = SetupDiGetDeviceInterfaceDetailW(set, &data, None, 0, Some(&mut required), None);
            if required < 8 {
                continue;
            }
            // SP_DEVICE_INTERFACE_DETAIL_DATA_W is a u32 size followed by the path's UTF-16 chars.
            let mut buf = vec![0u64; (required as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, Some(detail), required, None, None).is_ok() {
                let chars = (detail as *const u8).add(4) as *const u16;
                let mut path = Vec::new();
                let mut i = 0;
                while *chars.add(i) != 0 {
                    path.push(*chars.add(i));
                    i += 1;
                }
                path.push(0);
                paths.push(path);
            }
        }
        _ = SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

/// Is the ViGEmBus driver installed and running?
pub fn driver_present() -> bool {
    !interface_paths(&BUS_INTERFACE).is_empty()
}

pub struct Bus {
    handle: HANDLE,
}

// The handle is used from several threads; every call is a self-contained overlapped request.
unsafe impl Send for Bus {}
unsafe impl Sync for Bus {}

impl Drop for Bus {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.handle) };
    }
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// An Xbox 360 controller state (XINPUT_GAMEPAD layout).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct XReport {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

impl XReport {
    pub fn bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..2].copy_from_slice(&self.buttons.to_le_bytes());
        b[2] = self.left_trigger;
        b[3] = self.right_trigger;
        b[4..6].copy_from_slice(&self.lx.to_le_bytes());
        b[6..8].copy_from_slice(&self.ly.to_le_bytes());
        b[8..10].copy_from_slice(&self.rx.to_le_bytes());
        b[10..12].copy_from_slice(&self.ry.to_le_bytes());
        b
    }
}

impl Bus {
    pub fn open() -> Result<Bus, String> {
        let paths = interface_paths(&BUS_INTERFACE);
        let path = paths.first().ok_or("The ViGEmBus driver isn't installed")?;
        let flags = FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0 | FILE_FLAG_NO_BUFFERING.0 | FILE_FLAG_WRITE_THROUGH.0 | FILE_FLAG_OVERLAPPED.0);
        let handle = unsafe {
            CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, flags, None)
        }.map_err(|e| format!("Couldn't open the ViGEmBus driver: {e}"))?;
        let bus = Bus { handle };
        if !bus.ioctl(IOCTL_CHECK_VERSION, &words(&[8, COMMON_VERSION]), &mut []) {
            return Err("This ViGEmBus driver version isn't supported".into());
        }
        Ok(bus)
    }

    /// One request, waited for. `output` may be empty.
    fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> bool {
        unsafe {
            let Ok(event) = CreateEventW(None, false, false, PCWSTR::null()) else { return false };
            let mut overlapped = OVERLAPPED { hEvent: event, ..Default::default() };
            let out_ptr = if output.is_empty() { None } else { Some(output.as_mut_ptr() as *mut c_void) };
            // Usually returns ERROR_IO_PENDING; the result comes from GetOverlappedResult.
            _ = DeviceIoControl(self.handle, code, Some(input.as_ptr() as *const c_void), input.len() as u32, out_ptr, output.len() as u32, None, Some(&mut overlapped));
            let mut transferred = 0u32;
            let ok = GetOverlappedResult(self.handle, &overlapped, &mut transferred, true).is_ok();
            _ = CloseHandle(event);
            ok
        }
    }

    /// Plug in a virtual Xbox 360 controller; returns its serial (the slot on the bus).
    pub fn plug_x360(&self) -> Result<u32, String> {
        for serial in 1..=MAX_TARGETS {
            // VIGEM_PLUGIN_TARGET { Size, SerialNo, TargetType = Xbox360Wired, VendorId = 0, ProductId = 0 }
            let mut plugin = words(&[16, serial, 0]);
            plugin.extend_from_slice(&[0, 0, 0, 0]);
            if self.ioctl(IOCTL_PLUGIN_TARGET, &plugin, &mut []) {
                // Drivers since 1.17 report readiness separately; older ones reject this, which is fine.
                _ = self.ioctl(IOCTL_WAIT_DEVICE_READY, &words(&[8, serial]), &mut []);
                return Ok(serial);
            }
        }
        Err("No free virtual controller slot".into())
    }

    pub fn unplug(&self, serial: u32) {
        _ = self.ioctl(IOCTL_UNPLUG_TARGET, &words(&[8, serial]), &mut []);
    }

    pub fn submit(&self, serial: u32, report: &XReport) -> bool {
        let mut buf = words(&[20, serial]);
        buf.extend_from_slice(&report.bytes());
        self.ioctl(IOCTL_XUSB_SUBMIT_REPORT, &buf, &mut [])
    }

    /// Waits until the game changes rumble (or the pad is unplugged): (large motor, small motor, LED).
    pub fn wait_feedback(&self, serial: u32) -> Option<(u8, u8, u8)> {
        // XUSB_REQUEST_NOTIFICATION { Size = 12, SerialNo, LargeMotor, SmallMotor, LedNumber, pad }
        let mut buf = words(&[12, serial, 0]);
        let input = buf.clone();
        self.ioctl(IOCTL_XUSB_REQUEST_NOTIFICATION, &input, &mut buf).then(|| (buf[8], buf[9], buf[10]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_control_codes_match_vigem_client() {
        // Values of the macros in ViGEmClient's BusShared.h, worked out by CTL_CODE.
        assert_eq!(IOCTL_PLUGIN_TARGET, 0x2AA004);
        assert_eq!(IOCTL_UNPLUG_TARGET, 0x2AA008);
        assert_eq!(IOCTL_CHECK_VERSION, 0x2AA00C);
        assert_eq!(IOCTL_WAIT_DEVICE_READY, 0x2AA010);
        assert_eq!(IOCTL_XUSB_REQUEST_NOTIFICATION, 0x2AE804);
        assert_eq!(IOCTL_XUSB_SUBMIT_REPORT, 0x2AA808);
    }

    #[test]
    fn report_layout() {
        let r = XReport { buttons: 0x1001, left_trigger: 7, right_trigger: 9, lx: -2, ly: 3, rx: 4, ry: -5 };
        assert_eq!(r.bytes(), [0x01, 0x10, 7, 9, 0xFE, 0xFF, 3, 0, 4, 0, 0xFB, 0xFF]);
    }
}

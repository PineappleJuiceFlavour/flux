//! Flux's controller service: PlayStation controllers (DualSense, DualSense Edge, DualShock 4) show
//! up to games as Xbox 360 controllers, through the ViGEmBus driver. A background thread watches
//! for controllers; each one gets its own reader thread, a virtual Xbox pad, and a thread that sends
//! the game's rumble back to the real controller.

pub mod mapping;
pub mod pads;
pub mod vigem;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, RwLock, atomic::{AtomicBool, AtomicU8, Ordering}},
    time::Duration,
};

use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::{HIDD_ATTRIBUTES, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid},
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile},
        System::IO::CancelIoEx,
    },
    core::PCWSTR,
};

use mapping::Mapping;
use pads::Model;
use vigem::Bus;

/// Flux blue on the lightbar while a controller is mapped.
const LIGHTBAR: [u8; 3] = [0x56, 0x82, 0xFF];

#[derive(Debug, Clone, PartialEq)]
pub struct PadStatus {
    pub id: u64,
    pub name: &'static str,
    pub bluetooth: bool,
    pub battery: Option<u8>,
    pub charging: bool,
    /// The virtual Xbox controller's slot, once plugged in.
    pub xbox_slot: Option<u32>,
    /// The PlayStation buttons held right now (`PsButton::bit()`s), for the mapping editor.
    pub held: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub driver_installed: bool,
    pub pads: Vec<PadStatus>,
    pub error: Option<String>,
}

struct Service {
    enabled: AtomicBool,
    status: Mutex<Status>,
    /// Active controllers by HID device path.
    active: Mutex<HashMap<Vec<u16>, Arc<PadHandle>>>,
    mapping: RwLock<Mapping>,
}

struct PadHandle {
    read: HANDLE,
    stop: AtomicBool,
}

unsafe impl Send for PadHandle {}
unsafe impl Sync for PadHandle {}

fn service() -> &'static Service {
    static SERVICE: OnceLock<Service> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let service = Service { enabled: AtomicBool::new(false), status: Mutex::new(Status::default()), active: Mutex::new(HashMap::new()), mapping: RwLock::new(Mapping::default()) };
        std::thread::Builder::new().name("Flux controllers".into()).spawn(watch_loop).ok();
        service
    })
}

pub fn status() -> Status {
    service().status.lock().unwrap().clone()
}

/// Use this button mapping for every controller, from the next report on.
pub fn set_mapping(mapping: Mapping) {
    *service().mapping.write().unwrap() = mapping;
}

/// Turn PlayStation → Xbox controller emulation on or off.
pub fn set_enabled(enabled: bool) {
    let s = service();
    s.enabled.store(enabled, Ordering::SeqCst);
    s.status.lock().unwrap().enabled = enabled;
    if !enabled {
        for pad in s.active.lock().unwrap().values() {
            pad.stop.store(true, Ordering::SeqCst);
            unsafe { _ = CancelIoEx(pad.read, None) };
        }
    }
}

fn watch_loop() {
    // Give `service()` time to finish initialising before we use it.
    std::thread::sleep(Duration::from_millis(200));
    let s = service();
    let mut bus: Option<Arc<Bus>> = None;
    loop {
        let driver_installed = vigem::driver_present();
        {
            let mut status = s.status.lock().unwrap();
            status.driver_installed = driver_installed;
        }
        if s.enabled.load(Ordering::SeqCst) && driver_installed {
            if bus.is_none() {
                match Bus::open() {
                    Ok(b) => {
                        bus = Some(Arc::new(b));
                        s.status.lock().unwrap().error = None;
                    },
                    Err(e) => s.status.lock().unwrap().error = Some(e),
                }
            }
            if let Some(bus) = &bus {
                for (path, model) in find_controllers() {
                    if s.active.lock().unwrap().contains_key(&path) {
                        continue;
                    }
                    start_pad(path, model, bus.clone());
                }
            }
        } else if !s.enabled.load(Ordering::SeqCst) && s.active.lock().unwrap().is_empty() {
            bus = None;
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
}

fn open_hid(path: &[u16], access: u32) -> Option<HANDLE> {
    unsafe {
        CreateFileW(PCWSTR(path.as_ptr()), access, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None).ok()
    }
}

/// PlayStation controllers currently connected (one HID path each).
fn find_controllers() -> Vec<(Vec<u16>, Model)> {
    let class = unsafe { HidD_GetHidGuid() };
    vigem::interface_paths(&class).into_iter().filter_map(|path| {
        // Opening with no access is enough to read the ids, even for devices others hold open.
        let handle = open_hid(&path, 0)?;
        let mut attrs = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
        let ok = unsafe { HidD_GetAttributes(handle, &mut attrs) };
        unsafe { _ = CloseHandle(handle) };
        if !ok {
            return None;
        }
        Model::from_ids(attrs.VendorID, attrs.ProductID).map(|m| (path, m))
    }).collect()
}

fn start_pad(path: Vec<u16>, model: Model, bus: Arc<Bus>) {
    let s = service();
    let Some(read) = open_hid(&path, (GENERIC_READ | GENERIC_WRITE).0) else {
        s.status.lock().unwrap().error = Some(format!("Couldn't open the {}: another program may be using it exclusively", model.name()));
        return;
    };
    let write = open_hid(&path, (GENERIC_READ | GENERIC_WRITE).0).unwrap_or(read);
    let handle = Arc::new(PadHandle { read, stop: AtomicBool::new(false) });
    s.active.lock().unwrap().insert(path.clone(), handle.clone());
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let index = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    s.status.lock().unwrap().pads.push(PadStatus { id: index, name: model.name(), bluetooth: false, battery: None, charging: false, xbox_slot: None, held: 0 });

    let (read_handle, write_handle) = (WriteHandle(read), WriteHandle(write));
    std::thread::Builder::new().name("Flux pad".into()).spawn(move || {
        // Methods (not fields) so the closure takes the Send wrappers, not the raw handles.
        let (read, write) = (read_handle.get(), write_handle.get());
        // DualSense over Bluetooth sends short reports until its calibration feature report is read.
        let mut feature = [0u8; 41];
        feature[0] = 0x05;
        unsafe { _ = HidD_GetFeature(read, feature.as_mut_ptr() as _, feature.len() as u32) };

        let slot = bus.plug_x360().ok();
        let bluetooth = Arc::new(AtomicBool::new(false));
        let seq = Arc::new(AtomicU8::new(0));
        if let Some(slot) = slot {
            set_pad(index, |p| p.xbox_slot = Some(slot));
            // Rumble: the game's force feedback back to the real controller (lightbar stays Flux blue).
            let (bus2, bt2, seq2) = (bus.clone(), bluetooth.clone(), seq.clone());
            let rumble_handle = WriteHandle(write);
            std::thread::Builder::new().name("Flux rumble".into()).spawn(move || {
                let write = rumble_handle;
                send_output(&write, model, bt2.load(Ordering::SeqCst), &seq2, 0, 0);
                while let Some((large, small, _led)) = bus2.wait_feedback(slot) {
                    send_output(&write, model, bt2.load(Ordering::SeqCst), &seq2, large, small);
                }
            }).ok();
        }

        let mut buf = [0u8; 128];
        let mut last_status = std::time::Instant::now();
        let mut held = 0u32;
        let s = service();
        while !handle.stop.load(Ordering::SeqCst) {
            let mut read_len = 0u32;
            if unsafe { ReadFile(read, Some(&mut buf), Some(&mut read_len), None) }.is_err() {
                break; // unplugged, or stopped
            }
            let Some(input) = pads::parse(model, &buf[..read_len as usize]) else { continue };
            if input.bluetooth != bluetooth.swap(input.bluetooth, Ordering::SeqCst) {
                // First report (or the link changed): set the lightbar for this link type.
                send_output(&WriteHandle(write), model, input.bluetooth, &seq, 0, 0);
            }
            if let Some(slot) = slot {
                let report = s.mapping.read().unwrap().apply(&input.state);
                bus.submit(slot, &report);
            }
            if input.state.buttons != held {
                held = input.state.buttons;
                set_pad(index, |p| p.held = held);
            }
            if last_status.elapsed() > Duration::from_millis(500) {
                last_status = std::time::Instant::now();
                set_pad(index, |p| {
                    p.bluetooth = input.bluetooth;
                    if input.battery.is_some() {
                        p.battery = input.battery;
                    }
                    p.charging = input.charging;
                });
            }
        }

        if let Some(slot) = slot {
            bus.unplug(slot);
        }
        unsafe {
            _ = CloseHandle(read);
            if write != read {
                _ = CloseHandle(write);
            }
        }
        let s = service();
        s.active.lock().unwrap().remove(&path);
        s.status.lock().unwrap().pads.retain(|p| p.id != index);
    }).ok();
}

struct WriteHandle(HANDLE);
unsafe impl Send for WriteHandle {}

impl WriteHandle {
    fn get(&self) -> HANDLE {
        self.0
    }
}

fn send_output(write: &WriteHandle, model: Model, bluetooth: bool, seq: &AtomicU8, large: u8, small: u8) {
    let n = seq.fetch_add(1, Ordering::SeqCst);
    if let Some(report) = pads::output(model, bluetooth, n, large, small, LIGHTBAR) {
        unsafe { _ = WriteFile(write.0, Some(&report), None, None) };
    }
}

fn set_pad(id: u64, f: impl FnOnce(&mut PadStatus)) {
    if let Some(pad) = service().status.lock().unwrap().pads.iter_mut().find(|p| p.id == id) {
        f(pad);
    }
}

#[cfg(test)]
mod hardware {
    use super::*;

    /// FLUX_CONTROLLER_PROBE=1: report the driver and any PlayStation controllers, and decode a few
    /// seconds of real input. Read-only: no virtual controller is created.
    #[test]
    fn probe_real_hardware() {
        if std::env::var_os("FLUX_CONTROLLER_PROBE").is_none() {
            return;
        }
        println!("ViGEmBus driver installed: {}", vigem::driver_present());
        let found = find_controllers();
        println!("PlayStation controllers: {}", found.len());
        for (path, model) in found {
            println!("  {} at {}", model.name(), String::from_utf16_lossy(&path[..path.len() - 1]));
            let Some(read) = open_hid(&path, (GENERIC_READ | GENERIC_WRITE).0) else {
                println!("  (couldn't open it for reading)");
                continue;
            };
            let mut feature = [0u8; 41];
            feature[0] = 0x05;
            unsafe { _ = HidD_GetFeature(read, feature.as_mut_ptr() as _, feature.len() as u32) };
            let mut buf = [0u8; 128];
            let mut last = None;
            let start = std::time::Instant::now();
            let mut reports = 0;
            while start.elapsed() < Duration::from_secs(4) {
                let mut n = 0u32;
                if unsafe { ReadFile(read, Some(&mut buf), Some(&mut n), None) }.is_err() {
                    break;
                }
                reports += 1;
                if let Some(input) = pads::parse(model, &buf[..n as usize]) {
                    if last != Some(input.state) {
                        println!("    id {:#04x} len {n}: {:?} battery {:?}", buf[0], input.state, input.battery);
                        last = Some(input.state);
                    }
                }
            }
            println!("  {reports} reports in 4 s");
            unsafe { _ = CloseHandle(read) };
        }
    }
}

#[cfg(test)]
mod virtual_pad {
    use super::*;
    use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

    /// FLUX_VIGEM_TEST=1: plug a virtual Xbox 360 pad in for a moment, send a known state and read it
    /// back through XInput (what games use), then unplug it.
    #[test]
    fn xbox_pad_round_trip() {
        if std::env::var_os("FLUX_VIGEM_TEST").is_none() {
            return;
        }
        let bus = Bus::open().expect("open ViGEmBus");
        let slot = bus.plug_x360().expect("plug in a virtual Xbox 360 controller");
        println!("virtual Xbox 360 controller plugged in, bus slot {slot}");
        std::thread::sleep(Duration::from_millis(1500)); // let Windows bring the device up
        let sent = vigem::XReport { buttons: 0x1000, left_trigger: 128, right_trigger: 0, lx: 32767, ly: 0, rx: 0, ry: -12345 };
        let mut seen = None;
        for _ in 0..40 {
            assert!(bus.submit(slot, &sent), "submit report");
            std::thread::sleep(Duration::from_millis(50));
            for user in 0..4 {
                let mut state = XINPUT_STATE::default();
                if unsafe { XInputGetState(user, &mut state) } == 0 {
                    let g = state.Gamepad;
                    if g.wButtons.0 == sent.buttons && g.bLeftTrigger == sent.left_trigger && g.sThumbLX == sent.lx && g.sThumbRY == sent.ry {
                        seen = Some(user);
                    }
                }
            }
            if seen.is_some() {
                break;
            }
        }
        bus.unplug(slot);
        println!("XInput player slot that saw our exact state: {seen:?}");
        assert!(seen.is_some(), "a game reading XInput should see the virtual controller's state");
    }
}

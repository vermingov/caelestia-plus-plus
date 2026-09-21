//! A pointer for a compositor that has none.
//!
//! A headless compositor has no mouse, and a seat with no pointer never
//! tells a client where the pointer is, so nothing can be hovered. This
//! holds a virtual one open and does what it is told down a pipe:
//!
//!     move X Y | press | release | click | scroll DY
//!     key [ctrl+][shift+]NAME | type TEXT
//!
//! The keyboard is a virtual one too, with a US layout of its own: what is
//! typed at the rig is what the test says, whatever the machine's layout is.
//!
//! It has to stay running: the device goes when the client that made it
//! does, and the seat's pointer goes with it.

use std::io::{BufRead, BufReader, Seek, Write};
use std::os::fd::AsFd;
use std::time::{SystemTime, UNIX_EPOCH};

use wayland_client::protocol::{wl_keyboard, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

/// The panel the rig pretends to be.
const EXTENT: (u32, u32) = (1920, 1200);
const LEFT_BUTTON: u32 = 0x110;

#[derive(Default)]
struct Rig {
    manager: Option<ZwlrVirtualPointerManagerV1>,
    keyboards: Option<ZwpVirtualKeyboardManagerV1>,
    seat: Option<wl_seat::WlSeat>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Rig {
    fn event(
        rig: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global { name, interface, .. } = event else { return };
        match interface.as_str() {
            "zwlr_virtual_pointer_manager_v1" => rig.manager = Some(registry.bind(name, 1, queue, ())),
            "zwp_virtual_keyboard_manager_v1" => rig.keyboards = Some(registry.bind(name, 1, queue, ())),
            "wl_seat" => rig.seat = Some(registry.bind(name, 1, queue, ())),
            _ => {}
        }
    }
}

delegate_noop!(Rig: ZwlrVirtualPointerManagerV1);
delegate_noop!(Rig: ZwlrVirtualPointerV1);
delegate_noop!(Rig: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Rig: ZwpVirtualKeyboardV1);
delegate_noop!(Rig: ignore wl_seat::WlSeat);

/// Shift and Control, as bits of the modifier mask a US keymap uses.
const SHIFT: u32 = 1;
const CONTROL: u32 = 4;

/// The evdev code of a key, and whether Shift has to be down to get it.
fn code(name: &str) -> Option<(u32, bool)> {
    const LETTERS: &str = "qwertyuiop";
    const HOME: &str = "asdfghjkl";
    const BOTTOM: &str = "zxcvbnm";
    let plain = |code: u32| Some((code, false));
    match name {
        "esc" | "escape" => plain(1),
        "backspace" => plain(14),
        "tab" => plain(15),
        "enter" => plain(28),
        "space" | " " => plain(57),
        "up" => plain(103),
        "left" => plain(105),
        "right" => plain(106),
        "down" => plain(108),
        "home" => plain(102),
        "end" => plain(107),
        "delete" => plain(111),
        ">" => Some((52, true)),
        "." => plain(52),
        "," => plain(51),
        "-" => plain(12),
        "+" => Some((13, true)),
        "=" => plain(13),
        "*" => Some((9, true)),
        "/" => plain(53),
        "0" => plain(11),
        _ => {
            let character = name.chars().next().filter(|_| name.chars().count() == 1)?;
            if let Some(digit) = character.to_digit(10) {
                return plain(1 + digit);
            }
            let lower = character.to_ascii_lowercase();
            let code = LETTERS.find(lower).map(|at| 16 + at as u32)
                .or_else(|| HOME.find(lower).map(|at| 30 + at as u32))
                .or_else(|| BOTTOM.find(lower).map(|at| 44 + at as u32))?;
            Some((code, character.is_ascii_uppercase()))
        }
    }
}

/// One key, down and up again, with whatever is held while it is.
fn strike(keyboard: &ZwpVirtualKeyboardV1, code: u32, held: u32) {
    keyboard.modifiers(held, 0, 0, 0);
    keyboard.key(now(), code, wl_keyboard::KeyState::Pressed.into());
    keyboard.key(now(), code, wl_keyboard::KeyState::Released.into());
    keyboard.modifiers(0, 0, 0, 0);
}

fn press(keyboard: &ZwpVirtualKeyboardV1, line: &str) {
    let mut words = line.splitn(2, ' ');
    match (words.next(), words.next()) {
        (Some("key"), Some(chord)) => {
            let mut held = 0;
            let mut parts: Vec<&str> = chord.trim().split('+').collect();
            let name = parts.pop().unwrap_or_default();
            for part in parts {
                held |= match part { "ctrl" => CONTROL, "shift" => SHIFT, _ => 0 };
            }
            if let Some((code, shifted)) = code(name) {
                strike(keyboard, code, held | if shifted { SHIFT } else { 0 });
            }
        }
        (Some("type"), Some(text)) => {
            for character in text.chars() {
                if let Some((code, shifted)) = code(&character.to_string()) {
                    strike(keyboard, code, if shifted { SHIFT } else { 0 });
                }
            }
        }
        _ => {}
    }
}

fn now() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_millis() as u32).unwrap_or_default()
}

fn obey(pointer: &ZwlrVirtualPointerV1, line: &str) {
    let mut words = line.split_whitespace();
    let number = |word: Option<&str>| word.and_then(|word| word.parse::<f64>().ok());
    match words.next() {
        Some("move") => {
            let (Some(x), Some(y)) = (number(words.next()), number(words.next())) else { return };
            pointer.motion_absolute(now(), x as u32, y as u32, EXTENT.0, EXTENT.1);
        }
        Some("press") => pointer.button(now(), LEFT_BUTTON, wl_pointer::ButtonState::Pressed),
        Some("release") => pointer.button(now(), LEFT_BUTTON, wl_pointer::ButtonState::Released),
        Some("click") => {
            pointer.button(now(), LEFT_BUTTON, wl_pointer::ButtonState::Pressed);
            pointer.frame();
            pointer.button(now(), LEFT_BUTTON, wl_pointer::ButtonState::Released);
        }
        Some("scroll") => {
            let Some(distance) = number(words.next()) else { return };
            // As a wheel says it: a source, and whole clicks of it beside the
            // distance. A toolkit that knows it is listening to a wheel takes
            // the clicks and throws the bare distance away, which is what
            // GPUI does, and a scroll sent without them scrolls nothing.
            let clicks = ((distance / 15.).round() as i32).clamp(-20, 20);
            let clicks = if clicks == 0 { distance.signum() as i32 } else { clicks };
            pointer.axis_source(wl_pointer::AxisSource::Wheel);
            pointer.axis_discrete(now(), wl_pointer::Axis::VerticalScroll, distance, clicks);
        }
        _ => return,
    }
    pointer.frame();
}

/// A keyboard has to say what its keys mean before it may press one. The
/// keymap goes over as a file, which need not be on a disk.
fn keyboard(rig: &Rig, keymap: &str, handle: &QueueHandle<Rig>) -> Option<ZwpVirtualKeyboardV1> {
    let keyboard = rig.keyboards.as_ref()?.create_virtual_keyboard(rig.seat.as_ref()?, handle, ());
    let text = std::fs::read(keymap).ok()?;
    // SAFETY: a name and no flags; the descriptor is owned from here on.
    let mut file: std::fs::File = unsafe {
        use std::os::fd::FromRawFd;
        let fd = libc::memfd_create(c"rig-keymap".as_ptr(), 0);
        if fd < 0 {
            return None;
        }
        std::fs::File::from_raw_fd(fd)
    };
    file.write_all(&text).ok()?;
    file.write_all(&[0]).ok()?;
    file.rewind().ok()?;
    keyboard.keymap(wl_keyboard::KeymapFormat::XkbV1.into(), file.as_fd(), text.len() as u32 + 1);
    Some(keyboard)
}

fn main() {
    let pipe = std::env::args().nth(1).expect("usage: rig-pointer PIPE [KEYMAP]");
    let connection = Connection::connect_to_env().expect("no compositor to give a pointer to");
    let mut queue = connection.new_event_queue::<Rig>();
    let handle = queue.handle();
    connection.display().get_registry(&handle, ());

    let mut rig = Rig::default();
    queue.roundtrip(&mut rig).expect("the compositor went away");
    let manager = rig.manager.take().expect("this compositor has no virtual pointers");
    let pointer = manager.create_virtual_pointer(None, &handle, ());
    let keys = std::env::args().nth(2).and_then(|keymap| keyboard(&rig, &keymap, &handle));
    queue.roundtrip(&mut rig).expect("the compositor went away");

    // A pipe reads as finished whenever its last writer closes, which is
    // after every command: open it again and wait for the next.
    loop {
        let Ok(commands) = std::fs::File::open(&pipe) else { return };
        for line in BufReader::new(commands).lines().map_while(Result::ok) {
            match keys.as_ref().filter(|_| line.starts_with("key ") || line.starts_with("type ")) {
                Some(keys) => press(keys, &line),
                None => obey(&pointer, &line),
            }
            if queue.roundtrip(&mut rig).is_err() {
                return;
            }
        }
    }
}

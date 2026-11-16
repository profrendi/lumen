//! Konversi event winit -> event input Servo.

use std::str::FromStr;

use servo::{Code, Key, KeyState, KeyboardEvent, Location, Modifiers, MouseButton, NamedKey};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key as WKey, KeyLocation, ModifiersState, NamedKey as WNamed, PhysicalKey};

pub fn mouse_button(b: winit::event::MouseButton) -> MouseButton {
    use winit::event::MouseButton as W;
    match b {
        W::Left => MouseButton::Primary,
        W::Right => MouseButton::Secondary,
        W::Middle => MouseButton::Auxiliary,
        W::Back => MouseButton::Back,
        W::Forward => MouseButton::Forward,
        W::Other(n) => MouseButton::Other(n),
    }
}

pub fn modifiers(m: ModifiersState) -> Modifiers {
    let mut out = Modifiers::empty();
    out.set(Modifiers::CONTROL, m.control_key());
    out.set(Modifiers::SHIFT, m.shift_key());
    out.set(Modifiers::ALT, m.alt_key());
    out.set(Modifiers::META, m.super_key());
    out
}

fn named(k: WNamed) -> Option<Key> {
    use WNamed as W;
    let n = match k {
        W::Enter => NamedKey::Enter,
        W::Backspace => NamedKey::Backspace,
        W::Tab => NamedKey::Tab,
        W::Escape => NamedKey::Escape,
        W::Delete => NamedKey::Delete,
        W::Insert => NamedKey::Insert,
        W::ArrowLeft => NamedKey::ArrowLeft,
        W::ArrowRight => NamedKey::ArrowRight,
        W::ArrowUp => NamedKey::ArrowUp,
        W::ArrowDown => NamedKey::ArrowDown,
        W::Home => NamedKey::Home,
        W::End => NamedKey::End,
        W::PageUp => NamedKey::PageUp,
        W::PageDown => NamedKey::PageDown,
        W::Shift => NamedKey::Shift,
        W::Control => NamedKey::Control,
        W::Alt => NamedKey::Alt,
        W::Super => NamedKey::Meta,
        W::CapsLock => NamedKey::CapsLock,
        W::F1 => NamedKey::F1,
        W::F2 => NamedKey::F2,
        W::F3 => NamedKey::F3,
        W::F4 => NamedKey::F4,
        W::F5 => NamedKey::F5,
        W::F6 => NamedKey::F6,
        W::F7 => NamedKey::F7,
        W::F8 => NamedKey::F8,
        W::F9 => NamedKey::F9,
        W::F10 => NamedKey::F10,
        W::F11 => NamedKey::F11,
        W::F12 => NamedKey::F12,
        W::Space => return Some(Key::Character(" ".to_string())),
        _ => return None,
    };
    Some(Key::Named(n))
}

pub fn keyboard_event(ev: &KeyEvent, mods: ModifiersState) -> Option<KeyboardEvent> {
    let key = match &ev.logical_key {
        WKey::Character(s) => Key::Character(s.to_string()),
        WKey::Named(n) => named(*n)?,
        _ => return None,
    };
    // Nama KeyCode winit (mis. "KeyA", "ArrowLeft") sama dengan nama Code di standar W3C.
    let code = match ev.physical_key {
        PhysicalKey::Code(c) => Code::from_str(&format!("{c:?}")).unwrap_or(Code::Unidentified),
        PhysicalKey::Unidentified(_) => Code::Unidentified,
    };
    let location = match ev.location {
        KeyLocation::Standard => Location::Standard,
        KeyLocation::Left => Location::Left,
        KeyLocation::Right => Location::Right,
        KeyLocation::Numpad => Location::Numpad,
    };
    let state = match ev.state {
        ElementState::Pressed => KeyState::Down,
        ElementState::Released => KeyState::Up,
    };
    Some(KeyboardEvent::new_without_event(state, key, code, location, modifiers(mods), ev.repeat, false))
}

pub fn cursor_icon(c: servo::Cursor) -> winit::window::CursorIcon {
    use servo::Cursor as S;
    use winit::window::CursorIcon as W;
    match c {
        S::Default | S::None => W::Default,
        S::Pointer => W::Pointer,
        S::ContextMenu => W::ContextMenu,
        S::Help => W::Help,
        S::Progress => W::Progress,
        S::Wait => W::Wait,
        S::Cell => W::Cell,
        S::Crosshair => W::Crosshair,
        S::Text => W::Text,
        S::VerticalText => W::VerticalText,
        S::Alias => W::Alias,
        S::Copy => W::Copy,
        S::Move => W::Move,
        S::NoDrop => W::NoDrop,
        S::NotAllowed => W::NotAllowed,
        S::Grab => W::Grab,
        S::Grabbing => W::Grabbing,
        S::EResize => W::EResize,
        S::NResize => W::NResize,
        S::NeResize => W::NeResize,
        S::NwResize => W::NwResize,
        S::SResize => W::SResize,
        S::SeResize => W::SeResize,
        S::SwResize => W::SwResize,
        S::WResize => W::WResize,
        S::EwResize => W::EwResize,
        S::NsResize => W::NsResize,
        S::NeswResize => W::NeswResize,
        S::NwseResize => W::NwseResize,
        S::ColResize => W::ColResize,
        S::RowResize => W::RowResize,
        S::AllScroll => W::AllScroll,
        S::ZoomIn => W::ZoomIn,
        S::ZoomOut => W::ZoomOut,
    }
}

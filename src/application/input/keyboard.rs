//! Portable physical key catalog. Character entry/IME is a separate input concern.

/// A portable physical keyboard key supported by the runtime.
///
/// Desktop adapters translate platform key codes into this enum before input
/// reaches the headless runtime core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhysicalKeyCode {
    /// The physical W key.
    KeyW,
    /// The physical A key.
    KeyA,
    /// The physical S key.
    KeyS,
    /// The physical D key.
    KeyD,
    /// The main Enter key.
    Enter,
    /// The physical Space key.
    Space,
    /// The physical left-arrow key.
    ArrowLeft,
    /// The physical right-arrow key.
    ArrowRight,
    /// The physical down-arrow key.
    ArrowDown,
    /// The physical up-arrow key.
    ArrowUp,
    /// The physical Escape key.
    Escape,
    /// The physical P key.
    KeyP,
    /// The physical R key.
    KeyR,
    /// The physical N key.
    KeyN,
    /// The physical 1 key on the number row, not the numeric keypad.
    Digit1,
    /// The physical 2 key on the number row, not the numeric keypad.
    Digit2,
    /// The physical 3 key on the number row, not the numeric keypad.
    Digit3,
    /// The physical 4 key on the number row, not the numeric keypad.
    Digit4,
    /// The physical 5 key on the number row, not the numeric keypad.
    Digit5,
    /// The physical F3 function key.
    F3,
    /// The physical F4 function key.
    F4,
    /// The physical F5 function key.
    F5,
    /// The physical F6 function key.
    F6,
    /// The physical F8 function key.
    F8,
    /// The physical F9 function key.
    F9,
    /// The physical L key.
    KeyL,
    /// The physical F key.
    KeyF,
    /// The physical M key.
    KeyM,
    /// The physical T key.
    KeyT,
    /// The physical V key.
    KeyV,
    /// The physical 6 key on the number row.
    Digit6,
    /// The physical 7 key on the number row.
    Digit7,
    /// The physical 8 key on the number row.
    Digit8,
    /// The physical 9 key on the number row.
    Digit9,
    /// The physical F7 function key.
    F7,
    /// The physical E key.
    KeyE,
    /// The physical left Shift key.
    ShiftLeft,
    /// The physical Tab key (independent of keyboard layout).
    Tab,
    /// The physical Digit0 key (independent of keyboard layout).
    Digit0,
    /// The physical F1 key (independent of keyboard layout).
    F1,
    /// The physical F2 key (independent of keyboard layout).
    F2,
    /// The physical F10 key (independent of keyboard layout).
    F10,
    /// The physical F11 key (independent of keyboard layout).
    F11,
    /// The physical F12 key (independent of keyboard layout).
    F12,
    /// The physical ShiftRight key (independent of keyboard layout).
    ShiftRight,
    /// The physical ControlLeft key (independent of keyboard layout).
    ControlLeft,
    /// The physical ControlRight key (independent of keyboard layout).
    ControlRight,
    /// The physical AltLeft key (independent of keyboard layout).
    AltLeft,
    /// The physical AltRight key (independent of keyboard layout).
    AltRight,
    /// The physical SuperLeft key (independent of keyboard layout).
    SuperLeft,
    /// The physical SuperRight key (independent of keyboard layout).
    SuperRight,
    /// The physical Backspace key (independent of keyboard layout).
    Backspace,
    /// The physical Delete key (independent of keyboard layout).
    Delete,
    /// The physical Insert key (independent of keyboard layout).
    Insert,
    /// The physical Home key (independent of keyboard layout).
    Home,
    /// The physical End key (independent of keyboard layout).
    End,
    /// The physical PageUp key (independent of keyboard layout).
    PageUp,
    /// The physical PageDown key (independent of keyboard layout).
    PageDown,
    /// The physical CapsLock key (independent of keyboard layout).
    CapsLock,
    /// The physical NumLock key (independent of keyboard layout).
    NumLock,
    /// The physical ScrollLock key (independent of keyboard layout).
    ScrollLock,
    /// The physical PrintScreen key (independent of keyboard layout).
    PrintScreen,
    /// The physical Pause key (independent of keyboard layout).
    Pause,
    /// The physical ContextMenu key (independent of keyboard layout).
    ContextMenu,
    /// The physical Backquote key (independent of keyboard layout).
    Backquote,
    /// The physical Minus key (independent of keyboard layout).
    Minus,
    /// The physical Equal key (independent of keyboard layout).
    Equal,
    /// The physical BracketLeft key (independent of keyboard layout).
    BracketLeft,
    /// The physical BracketRight key (independent of keyboard layout).
    BracketRight,
    /// The physical Backslash key (independent of keyboard layout).
    Backslash,
    /// The physical Semicolon key (independent of keyboard layout).
    Semicolon,
    /// The physical Quote key (independent of keyboard layout).
    Quote,
    /// The physical Comma key (independent of keyboard layout).
    Comma,
    /// The physical Period key (independent of keyboard layout).
    Period,
    /// The physical Slash key (independent of keyboard layout).
    Slash,
    /// The physical KeyB key (independent of keyboard layout).
    KeyB,
    /// The physical KeyC key (independent of keyboard layout).
    KeyC,
    /// The physical KeyG key (independent of keyboard layout).
    KeyG,
    /// The physical KeyH key (independent of keyboard layout).
    KeyH,
    /// The physical KeyI key (independent of keyboard layout).
    KeyI,
    /// The physical KeyJ key (independent of keyboard layout).
    KeyJ,
    /// The physical KeyK key (independent of keyboard layout).
    KeyK,
    /// The physical KeyO key (independent of keyboard layout).
    KeyO,
    /// The physical KeyU key (independent of keyboard layout).
    KeyU,
    /// The physical KeyX key (independent of keyboard layout).
    KeyX,
    /// The physical KeyY key (independent of keyboard layout).
    KeyY,
    /// The physical KeyZ key (independent of keyboard layout).
    KeyZ,
    /// The physical Numpad0 key (independent of keyboard layout).
    Numpad0,
    /// The physical Numpad1 key (independent of keyboard layout).
    Numpad1,
    /// The physical Numpad2 key (independent of keyboard layout).
    Numpad2,
    /// The physical Numpad3 key (independent of keyboard layout).
    Numpad3,
    /// The physical Numpad4 key (independent of keyboard layout).
    Numpad4,
    /// The physical Numpad5 key (independent of keyboard layout).
    Numpad5,
    /// The physical Numpad6 key (independent of keyboard layout).
    Numpad6,
    /// The physical Numpad7 key (independent of keyboard layout).
    Numpad7,
    /// The physical Numpad8 key (independent of keyboard layout).
    Numpad8,
    /// The physical Numpad9 key (independent of keyboard layout).
    Numpad9,
    /// The physical NumpadEnter key (independent of keyboard layout).
    NumpadEnter,
    /// The physical NumpadAdd key (independent of keyboard layout).
    NumpadAdd,
    /// The physical NumpadSubtract key (independent of keyboard layout).
    NumpadSubtract,
    /// The physical NumpadMultiply key (independent of keyboard layout).
    NumpadMultiply,
    /// The physical NumpadDivide key (independent of keyboard layout).
    NumpadDivide,
    /// The physical NumpadDecimal key (independent of keyboard layout).
    NumpadDecimal,
    /// The physical NumpadEqual key (independent of keyboard layout).
    NumpadEqual,
    /// The physical IntlBackslash key (independent of keyboard layout).
    IntlBackslash,
    /// The physical IntlRo key (independent of keyboard layout).
    IntlRo,
    /// The physical IntlYen key (independent of keyboard layout).
    IntlYen,
    /// The physical Q key, independent of keyboard layout.
    KeyQ,
}

pub(crate) const ALL_PHYSICAL_KEYS: [PhysicalKeyCode; 108] = [
    PhysicalKeyCode::KeyW,
    PhysicalKeyCode::KeyA,
    PhysicalKeyCode::KeyS,
    PhysicalKeyCode::KeyD,
    PhysicalKeyCode::Enter,
    PhysicalKeyCode::Space,
    PhysicalKeyCode::ArrowLeft,
    PhysicalKeyCode::ArrowRight,
    PhysicalKeyCode::ArrowDown,
    PhysicalKeyCode::ArrowUp,
    PhysicalKeyCode::Escape,
    PhysicalKeyCode::KeyP,
    PhysicalKeyCode::KeyR,
    PhysicalKeyCode::KeyN,
    PhysicalKeyCode::Digit1,
    PhysicalKeyCode::Digit2,
    PhysicalKeyCode::Digit3,
    PhysicalKeyCode::Digit4,
    PhysicalKeyCode::Digit5,
    PhysicalKeyCode::F3,
    PhysicalKeyCode::F4,
    PhysicalKeyCode::F5,
    PhysicalKeyCode::F6,
    PhysicalKeyCode::F8,
    PhysicalKeyCode::F9,
    PhysicalKeyCode::KeyL,
    PhysicalKeyCode::KeyF,
    PhysicalKeyCode::KeyM,
    PhysicalKeyCode::KeyT,
    PhysicalKeyCode::KeyV,
    PhysicalKeyCode::Digit6,
    PhysicalKeyCode::Digit7,
    PhysicalKeyCode::Digit8,
    PhysicalKeyCode::Digit9,
    PhysicalKeyCode::F7,
    PhysicalKeyCode::KeyE,
    PhysicalKeyCode::ShiftLeft,
    PhysicalKeyCode::Tab,
    PhysicalKeyCode::Digit0,
    PhysicalKeyCode::F1,
    PhysicalKeyCode::F2,
    PhysicalKeyCode::F10,
    PhysicalKeyCode::F11,
    PhysicalKeyCode::F12,
    PhysicalKeyCode::ShiftRight,
    PhysicalKeyCode::ControlLeft,
    PhysicalKeyCode::ControlRight,
    PhysicalKeyCode::AltLeft,
    PhysicalKeyCode::AltRight,
    PhysicalKeyCode::SuperLeft,
    PhysicalKeyCode::SuperRight,
    PhysicalKeyCode::Backspace,
    PhysicalKeyCode::Delete,
    PhysicalKeyCode::Insert,
    PhysicalKeyCode::Home,
    PhysicalKeyCode::End,
    PhysicalKeyCode::PageUp,
    PhysicalKeyCode::PageDown,
    PhysicalKeyCode::CapsLock,
    PhysicalKeyCode::NumLock,
    PhysicalKeyCode::ScrollLock,
    PhysicalKeyCode::PrintScreen,
    PhysicalKeyCode::Pause,
    PhysicalKeyCode::ContextMenu,
    PhysicalKeyCode::Backquote,
    PhysicalKeyCode::Minus,
    PhysicalKeyCode::Equal,
    PhysicalKeyCode::BracketLeft,
    PhysicalKeyCode::BracketRight,
    PhysicalKeyCode::Backslash,
    PhysicalKeyCode::Semicolon,
    PhysicalKeyCode::Quote,
    PhysicalKeyCode::Comma,
    PhysicalKeyCode::Period,
    PhysicalKeyCode::Slash,
    PhysicalKeyCode::KeyB,
    PhysicalKeyCode::KeyC,
    PhysicalKeyCode::KeyG,
    PhysicalKeyCode::KeyH,
    PhysicalKeyCode::KeyI,
    PhysicalKeyCode::KeyJ,
    PhysicalKeyCode::KeyK,
    PhysicalKeyCode::KeyO,
    PhysicalKeyCode::KeyU,
    PhysicalKeyCode::KeyX,
    PhysicalKeyCode::KeyY,
    PhysicalKeyCode::KeyZ,
    PhysicalKeyCode::Numpad0,
    PhysicalKeyCode::Numpad1,
    PhysicalKeyCode::Numpad2,
    PhysicalKeyCode::Numpad3,
    PhysicalKeyCode::Numpad4,
    PhysicalKeyCode::Numpad5,
    PhysicalKeyCode::Numpad6,
    PhysicalKeyCode::Numpad7,
    PhysicalKeyCode::Numpad8,
    PhysicalKeyCode::Numpad9,
    PhysicalKeyCode::NumpadEnter,
    PhysicalKeyCode::NumpadAdd,
    PhysicalKeyCode::NumpadSubtract,
    PhysicalKeyCode::NumpadMultiply,
    PhysicalKeyCode::NumpadDivide,
    PhysicalKeyCode::NumpadDecimal,
    PhysicalKeyCode::NumpadEqual,
    PhysicalKeyCode::IntlBackslash,
    PhysicalKeyCode::IntlRo,
    PhysicalKeyCode::IntlYen,
    PhysicalKeyCode::KeyQ,
];

pub(crate) const SUPPORTED_PHYSICAL_KEY_COUNT: usize = ALL_PHYSICAL_KEYS.len();

pub(crate) const fn physical_key_index(key: PhysicalKeyCode) -> usize {
    match key {
        PhysicalKeyCode::KeyW => 0,
        PhysicalKeyCode::KeyA => 1,
        PhysicalKeyCode::KeyS => 2,
        PhysicalKeyCode::KeyD => 3,
        PhysicalKeyCode::Enter => 4,
        PhysicalKeyCode::Space => 5,
        PhysicalKeyCode::ArrowLeft => 6,
        PhysicalKeyCode::ArrowRight => 7,
        PhysicalKeyCode::ArrowDown => 8,
        PhysicalKeyCode::ArrowUp => 9,
        PhysicalKeyCode::Escape => 10,
        PhysicalKeyCode::KeyP => 11,
        PhysicalKeyCode::KeyR => 12,
        PhysicalKeyCode::KeyN => 13,
        PhysicalKeyCode::Digit1 => 14,
        PhysicalKeyCode::Digit2 => 15,
        PhysicalKeyCode::Digit3 => 16,
        PhysicalKeyCode::Digit4 => 17,
        PhysicalKeyCode::Digit5 => 18,
        PhysicalKeyCode::F3 => 19,
        PhysicalKeyCode::F4 => 20,
        PhysicalKeyCode::F5 => 21,
        PhysicalKeyCode::F6 => 22,
        PhysicalKeyCode::F8 => 23,
        PhysicalKeyCode::F9 => 24,
        PhysicalKeyCode::KeyL => 25,
        PhysicalKeyCode::KeyF => 26,
        PhysicalKeyCode::KeyM => 27,
        PhysicalKeyCode::KeyT => 28,
        PhysicalKeyCode::KeyV => 29,
        PhysicalKeyCode::Digit6 => 30,
        PhysicalKeyCode::Digit7 => 31,
        PhysicalKeyCode::Digit8 => 32,
        PhysicalKeyCode::Digit9 => 33,
        PhysicalKeyCode::F7 => 34,
        PhysicalKeyCode::KeyE => 35,
        PhysicalKeyCode::ShiftLeft => 36,
        PhysicalKeyCode::Tab => 37,
        PhysicalKeyCode::Digit0 => 38,
        PhysicalKeyCode::F1 => 39,
        PhysicalKeyCode::F2 => 40,
        PhysicalKeyCode::F10 => 41,
        PhysicalKeyCode::F11 => 42,
        PhysicalKeyCode::F12 => 43,
        PhysicalKeyCode::ShiftRight => 44,
        PhysicalKeyCode::ControlLeft => 45,
        PhysicalKeyCode::ControlRight => 46,
        PhysicalKeyCode::AltLeft => 47,
        PhysicalKeyCode::AltRight => 48,
        PhysicalKeyCode::SuperLeft => 49,
        PhysicalKeyCode::SuperRight => 50,
        PhysicalKeyCode::Backspace => 51,
        PhysicalKeyCode::Delete => 52,
        PhysicalKeyCode::Insert => 53,
        PhysicalKeyCode::Home => 54,
        PhysicalKeyCode::End => 55,
        PhysicalKeyCode::PageUp => 56,
        PhysicalKeyCode::PageDown => 57,
        PhysicalKeyCode::CapsLock => 58,
        PhysicalKeyCode::NumLock => 59,
        PhysicalKeyCode::ScrollLock => 60,
        PhysicalKeyCode::PrintScreen => 61,
        PhysicalKeyCode::Pause => 62,
        PhysicalKeyCode::ContextMenu => 63,
        PhysicalKeyCode::Backquote => 64,
        PhysicalKeyCode::Minus => 65,
        PhysicalKeyCode::Equal => 66,
        PhysicalKeyCode::BracketLeft => 67,
        PhysicalKeyCode::BracketRight => 68,
        PhysicalKeyCode::Backslash => 69,
        PhysicalKeyCode::Semicolon => 70,
        PhysicalKeyCode::Quote => 71,
        PhysicalKeyCode::Comma => 72,
        PhysicalKeyCode::Period => 73,
        PhysicalKeyCode::Slash => 74,
        PhysicalKeyCode::KeyB => 75,
        PhysicalKeyCode::KeyC => 76,
        PhysicalKeyCode::KeyG => 77,
        PhysicalKeyCode::KeyH => 78,
        PhysicalKeyCode::KeyI => 79,
        PhysicalKeyCode::KeyJ => 80,
        PhysicalKeyCode::KeyK => 81,
        PhysicalKeyCode::KeyO => 82,
        PhysicalKeyCode::KeyU => 83,
        PhysicalKeyCode::KeyX => 84,
        PhysicalKeyCode::KeyY => 85,
        PhysicalKeyCode::KeyZ => 86,
        PhysicalKeyCode::Numpad0 => 87,
        PhysicalKeyCode::Numpad1 => 88,
        PhysicalKeyCode::Numpad2 => 89,
        PhysicalKeyCode::Numpad3 => 90,
        PhysicalKeyCode::Numpad4 => 91,
        PhysicalKeyCode::Numpad5 => 92,
        PhysicalKeyCode::Numpad6 => 93,
        PhysicalKeyCode::Numpad7 => 94,
        PhysicalKeyCode::Numpad8 => 95,
        PhysicalKeyCode::Numpad9 => 96,
        PhysicalKeyCode::NumpadEnter => 97,
        PhysicalKeyCode::NumpadAdd => 98,
        PhysicalKeyCode::NumpadSubtract => 99,
        PhysicalKeyCode::NumpadMultiply => 100,
        PhysicalKeyCode::NumpadDivide => 101,
        PhysicalKeyCode::NumpadDecimal => 102,
        PhysicalKeyCode::NumpadEqual => 103,
        PhysicalKeyCode::IntlBackslash => 104,
        PhysicalKeyCode::IntlRo => 105,
        PhysicalKeyCode::IntlYen => 106,
        PhysicalKeyCode::KeyQ => 107,
    }
}

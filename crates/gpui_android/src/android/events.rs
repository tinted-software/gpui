//! Conversion from Android NDK / `android-activity` input events to GPUI input events.

use android_activity::input::{
    InputEvent, KeyAction, KeyEvent, KeyMapChar, Keycode, MetaState, MotionAction, MotionEvent,
};
use gpui::{
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, Pixels, PlatformInput, Point, TouchEvent,
    TouchId, TouchPhase, px,
};

pub(crate) fn convert_input_event(
    event: &InputEvent<'_>,
    scale_factor: f32,
    key_character_map: Option<&android_activity::input::KeyCharacterMap>,
) -> Vec<PlatformInput> {
    match event {
        InputEvent::MotionEvent(motion) => convert_motion_event(motion, scale_factor),
        InputEvent::KeyEvent(key) => convert_key_event(key, key_character_map)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn convert_motion_event(event: &MotionEvent<'_>, scale_factor: f32) -> Vec<PlatformInput> {
    let scale = scale_factor.max(1.0);
    let action = event.action();
    let pointer_position = |pointer: &android_activity::input::Pointer<'_>| -> Point<Pixels> {
        Point::new(px(pointer.x() / scale), px(pointer.y() / scale))
    };

    match action {
        MotionAction::Down => {
            let pointer = event.pointer_at_index(0);
            vec![PlatformInput::Touch(TouchEvent {
                id: TouchId(pointer.pointer_id() as u64),
                phase: TouchPhase::Started,
                position: pointer_position(&pointer),
                predicted_position: None,
                force: Some(pointer.pressure().clamp(0.0, 1.0)),
            })]
        }
        MotionAction::PointerDown => {
            let pointer = event.pointer_at_index(event.pointer_index());
            vec![PlatformInput::Touch(TouchEvent {
                id: TouchId(pointer.pointer_id() as u64),
                phase: TouchPhase::Started,
                position: pointer_position(&pointer),
                predicted_position: None,
                force: Some(pointer.pressure().clamp(0.0, 1.0)),
            })]
        }
        MotionAction::Move => event
            .pointers()
            .map(|pointer| {
                PlatformInput::Touch(TouchEvent {
                    id: TouchId(pointer.pointer_id() as u64),
                    phase: TouchPhase::Moved,
                    position: pointer_position(&pointer),
                    predicted_position: None,
                    force: Some(pointer.pressure().clamp(0.0, 1.0)),
                })
            })
            .collect(),
        MotionAction::Up => {
            let pointer = event.pointer_at_index(0);
            vec![PlatformInput::Touch(TouchEvent {
                id: TouchId(pointer.pointer_id() as u64),
                phase: TouchPhase::Ended,
                position: pointer_position(&pointer),
                predicted_position: None,
                force: Some(pointer.pressure().clamp(0.0, 1.0)),
            })]
        }
        MotionAction::PointerUp => {
            let pointer = event.pointer_at_index(event.pointer_index());
            vec![PlatformInput::Touch(TouchEvent {
                id: TouchId(pointer.pointer_id() as u64),
                phase: TouchPhase::Ended,
                position: pointer_position(&pointer),
                predicted_position: None,
                force: Some(pointer.pressure().clamp(0.0, 1.0)),
            })]
        }
        MotionAction::Cancel => event
            .pointers()
            .map(|pointer| {
                PlatformInput::Touch(TouchEvent {
                    id: TouchId(pointer.pointer_id() as u64),
                    phase: TouchPhase::Cancelled,
                    position: pointer_position(&pointer),
                    predicted_position: None,
                    force: Some(pointer.pressure().clamp(0.0, 1.0)),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn convert_key_event(
    event: &KeyEvent<'_>,
    key_character_map: Option<&android_activity::input::KeyCharacterMap>,
) -> Option<PlatformInput> {
    let modifiers = convert_modifiers(event.meta_state());
    let key = keycode_to_gpui_key(event.key_code())?;
    let key_char = key_character_map
        .and_then(|map| {
            map.get(event.key_code(), event.meta_state())
                .ok()
                .and_then(|ch| match ch {
                    KeyMapChar::Unicode(c) if !c.is_control() => Some(c.to_string()),
                    _ => None,
                })
        })
        .or_else(|| default_key_char(event.key_code(), modifiers.shift));

    let keystroke = Keystroke {
        modifiers,
        key: key.to_string(),
        key_char,
    };

    match event.action() {
        KeyAction::Down => Some(PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: event.repeat_count() > 0,
            prefer_character_input: false,
        })),
        KeyAction::Up => Some(PlatformInput::KeyUp(KeyUpEvent { keystroke })),
        _ => None,
    }
}

fn convert_modifiers(meta: MetaState) -> Modifiers {
    Modifiers {
        control: meta.ctrl_on(),
        alt: meta.alt_on(),
        shift: meta.shift_on(),
        platform: meta.meta_on(),
        function: meta.function_on(),
    }
}

fn keycode_to_gpui_key(keycode: Keycode) -> Option<&'static str> {
    match keycode {
        Keycode::A => Some("a"),
        Keycode::B => Some("b"),
        Keycode::C => Some("c"),
        Keycode::D => Some("d"),
        Keycode::E => Some("e"),
        Keycode::F => Some("f"),
        Keycode::G => Some("g"),
        Keycode::H => Some("h"),
        Keycode::I => Some("i"),
        Keycode::J => Some("j"),
        Keycode::K => Some("k"),
        Keycode::L => Some("l"),
        Keycode::M => Some("m"),
        Keycode::N => Some("n"),
        Keycode::O => Some("o"),
        Keycode::P => Some("p"),
        Keycode::Q => Some("q"),
        Keycode::R => Some("r"),
        Keycode::S => Some("s"),
        Keycode::T => Some("t"),
        Keycode::U => Some("u"),
        Keycode::V => Some("v"),
        Keycode::W => Some("w"),
        Keycode::X => Some("x"),
        Keycode::Y => Some("y"),
        Keycode::Z => Some("z"),
        Keycode::Keycode0 => Some("0"),
        Keycode::Keycode1 => Some("1"),
        Keycode::Keycode2 => Some("2"),
        Keycode::Keycode3 => Some("3"),
        Keycode::Keycode4 => Some("4"),
        Keycode::Keycode5 => Some("5"),
        Keycode::Keycode6 => Some("6"),
        Keycode::Keycode7 => Some("7"),
        Keycode::Keycode8 => Some("8"),
        Keycode::Keycode9 => Some("9"),
        Keycode::Enter | Keycode::NumpadEnter => Some("enter"),
        Keycode::Tab => Some("tab"),
        Keycode::Space => Some("space"),
        Keycode::Del => Some("backspace"),
        Keycode::ForwardDel => Some("delete"),
        Keycode::Escape | Keycode::Back => Some("escape"),
        Keycode::DpadUp => Some("up"),
        Keycode::DpadDown => Some("down"),
        Keycode::DpadLeft => Some("left"),
        Keycode::DpadRight => Some("right"),
        Keycode::MoveHome => Some("home"),
        Keycode::MoveEnd => Some("end"),
        Keycode::PageUp => Some("pageup"),
        Keycode::PageDown => Some("pagedown"),
        Keycode::Minus => Some("-"),
        Keycode::Equals => Some("="),
        Keycode::LeftBracket => Some("["),
        Keycode::RightBracket => Some("]"),
        Keycode::Backslash => Some("\\"),
        Keycode::Semicolon => Some(";"),
        Keycode::Apostrophe => Some("'"),
        Keycode::Comma => Some(","),
        Keycode::Period => Some("."),
        Keycode::Slash => Some("/"),
        Keycode::Grave => Some("`"),
        _ => None,
    }
}

fn default_key_char(keycode: Keycode, shift: bool) -> Option<String> {
    let ch = match keycode {
        Keycode::Space => ' ',
        Keycode::A => {
            if shift {
                'A'
            } else {
                'a'
            }
        }
        Keycode::B => {
            if shift {
                'B'
            } else {
                'b'
            }
        }
        Keycode::C => {
            if shift {
                'C'
            } else {
                'c'
            }
        }
        Keycode::D => {
            if shift {
                'D'
            } else {
                'd'
            }
        }
        Keycode::E => {
            if shift {
                'E'
            } else {
                'e'
            }
        }
        Keycode::F => {
            if shift {
                'F'
            } else {
                'f'
            }
        }
        Keycode::G => {
            if shift {
                'G'
            } else {
                'g'
            }
        }
        Keycode::H => {
            if shift {
                'H'
            } else {
                'h'
            }
        }
        Keycode::I => {
            if shift {
                'I'
            } else {
                'i'
            }
        }
        Keycode::J => {
            if shift {
                'J'
            } else {
                'j'
            }
        }
        Keycode::K => {
            if shift {
                'K'
            } else {
                'k'
            }
        }
        Keycode::L => {
            if shift {
                'L'
            } else {
                'l'
            }
        }
        Keycode::M => {
            if shift {
                'M'
            } else {
                'm'
            }
        }
        Keycode::N => {
            if shift {
                'N'
            } else {
                'n'
            }
        }
        Keycode::O => {
            if shift {
                'O'
            } else {
                'o'
            }
        }
        Keycode::P => {
            if shift {
                'P'
            } else {
                'p'
            }
        }
        Keycode::Q => {
            if shift {
                'Q'
            } else {
                'q'
            }
        }
        Keycode::R => {
            if shift {
                'R'
            } else {
                'r'
            }
        }
        Keycode::S => {
            if shift {
                'S'
            } else {
                's'
            }
        }
        Keycode::T => {
            if shift {
                'T'
            } else {
                't'
            }
        }
        Keycode::U => {
            if shift {
                'U'
            } else {
                'u'
            }
        }
        Keycode::V => {
            if shift {
                'V'
            } else {
                'v'
            }
        }
        Keycode::W => {
            if shift {
                'W'
            } else {
                'w'
            }
        }
        Keycode::X => {
            if shift {
                'X'
            } else {
                'x'
            }
        }
        Keycode::Y => {
            if shift {
                'Y'
            } else {
                'y'
            }
        }
        Keycode::Z => {
            if shift {
                'Z'
            } else {
                'z'
            }
        }
        Keycode::Keycode0 => {
            if shift {
                ')'
            } else {
                '0'
            }
        }
        Keycode::Keycode1 => {
            if shift {
                '!'
            } else {
                '1'
            }
        }
        Keycode::Keycode2 => {
            if shift {
                '@'
            } else {
                '2'
            }
        }
        Keycode::Keycode3 => {
            if shift {
                '#'
            } else {
                '3'
            }
        }
        Keycode::Keycode4 => {
            if shift {
                '$'
            } else {
                '4'
            }
        }
        Keycode::Keycode5 => {
            if shift {
                '%'
            } else {
                '5'
            }
        }
        Keycode::Keycode6 => {
            if shift {
                '^'
            } else {
                '6'
            }
        }
        Keycode::Keycode7 => {
            if shift {
                '&'
            } else {
                '7'
            }
        }
        Keycode::Keycode8 => {
            if shift {
                '*'
            } else {
                '8'
            }
        }
        Keycode::Keycode9 => {
            if shift {
                '('
            } else {
                '9'
            }
        }
        _ => return None,
    };
    Some(ch.to_string())
}

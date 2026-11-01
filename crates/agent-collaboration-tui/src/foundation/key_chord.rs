//! Pure parser adapted from CometixTUI, MIT, ec7c0615, use_keybinding.rs:1–153.
//! Its subscription and propagation defaults are deliberately not borrowed.
use iocraft::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyChordError(String);
impl fmt::Display for KeyChordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for KeyChordError {}

impl KeyChord {
    pub(crate) fn normalized(code: KeyCode, mut modifiers: KeyModifiers) -> Self {
        let code = match code {
            KeyCode::Char('\u{1f}') => {
                modifiers |= KeyModifiers::CONTROL;
                KeyCode::Char('7')
            }
            KeyCode::Char('\u{3}') if modifiers.is_empty() => {
                modifiers |= KeyModifiers::CONTROL;
                KeyCode::Char('c')
            }
            KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
            KeyCode::BackTab => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            other => other,
        };
        Self { code, modifiers }
    }

    /// Effectful actions accept Press only; editing repeats belong to the target.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        event.kind == KeyEventKind::Press && *self == Self::normalized(event.code, event.modifiers)
    }
}

impl FromStr for KeyChord {
    type Err = KeyChordError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut modifiers = KeyModifiers::empty();
        let mut code = None;
        for part in input.split('+').map(str::trim) {
            let lower = part.to_ascii_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "option" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                "super" | "cmd" | "meta" | "win" => modifiers |= KeyModifiers::SUPER,
                _ => {
                    if code.is_some() {
                        return Err(KeyChordError(format!("multiple keys in {input:?}")));
                    }
                    code = Some(parse_key_code(&lower, part)?);
                }
            }
        }
        code.map(|code| Self::normalized(code, modifiers))
            .ok_or_else(|| KeyChordError(format!("missing key in {input:?}")))
    }
}

fn parse_key_code(lower: &str, original: &str) -> Result<KeyCode, KeyChordError> {
    Ok(match lower {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "space" => KeyCode::Char(' '),
        "plus" => KeyCode::Char('+'),
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        _ => {
            if let Some(number) = lower
                .strip_prefix('f')
                .and_then(|part| part.parse::<u8>().ok())
                && (1..=24).contains(&number)
            {
                return Ok(KeyCode::F(number));
            }
            let mut characters = original.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => KeyCode::Char(character),
                _ => return Err(KeyChordError(format!("unknown key {original:?}"))),
            }
        }
    })
}

impl fmt::Display for KeyChord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, label) in [
            (KeyModifiers::CONTROL, "Ctrl"),
            (KeyModifiers::ALT, "Alt"),
            (KeyModifiers::SHIFT, "Shift"),
            (KeyModifiers::SUPER, "Super"),
        ] {
            if self.modifiers.contains(modifier) {
                write!(formatter, "{label}+")?;
            }
        }
        match self.code {
            KeyCode::Enter => formatter.write_str("Enter"),
            KeyCode::Esc => formatter.write_str("Esc"),
            KeyCode::Char(' ') => formatter.write_str("Space"),
            KeyCode::Char(character) => write!(formatter, "{}", character.to_ascii_uppercase()),
            code => write!(formatter, "{code}"),
        }
    }
}

#[cfg(test)]
#[path = "key_chord_test.rs"]
mod tests;

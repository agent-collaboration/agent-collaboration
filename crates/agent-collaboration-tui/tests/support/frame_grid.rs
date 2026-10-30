//! Closed grammar of pinned Canvas::write_ansi; never a permissive VT parser.
use iocraft::{Canvas, Color};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrameStyle {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub invert: bool,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrameCell {
    pub text: String,
    pub continuation: bool,
    pub style: FrameStyle,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Vec<FrameCell>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameError {
    UnknownEscape(String),
    InvalidSgr(String),
    InvalidControl(char),
    Bounds,
    Utf8,
    Write,
}

impl FrameGrid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![vec![FrameCell::default(); width]; height],
        }
    }
    pub fn from_canvas(canvas: &Canvas) -> Result<Self, FrameError> {
        super::require_color_environment();
        let mut bytes = Vec::new();
        canvas
            .write_ansi(&mut bytes)
            .map_err(|_| FrameError::Write)?;
        let mut grid = Self::new(canvas.width(), canvas.height());
        grid.apply(std::str::from_utf8(&bytes).map_err(|_| FrameError::Utf8)?)?;
        Ok(grid)
    }
    pub fn cell(&self, column: usize, row: usize) -> Option<&FrameCell> {
        self.cells.get(row)?.get(column)
    }
    pub fn row_text(&self, row: usize) -> String {
        self.cells
            .get(row)
            .into_iter()
            .flatten()
            .map(|cell| {
                if cell.continuation {
                    ""
                } else if cell.text.is_empty() {
                    " "
                } else {
                    cell.text.as_str()
                }
            })
            .collect()
    }
    pub fn apply(&mut self, input: &str) -> Result<(), FrameError> {
        let mut characters = input.chars().peekable();
        let (mut column, mut row) = (0, 0);
        let mut style = FrameStyle::default();
        while let Some(character) = characters.next() {
            match character {
                '\x1b' => {
                    if characters.next() != Some('[') {
                        return Err(FrameError::UnknownEscape(
                            "non-CSI escape (including OSC 8)".into(),
                        ));
                    }
                    let mut parameters = String::new();
                    let command = loop {
                        let next = characters
                            .next()
                            .ok_or_else(|| FrameError::UnknownEscape(parameters.clone()))?;
                        if ('@'..='~').contains(&next) {
                            break next;
                        }
                        parameters.push(next);
                    };
                    match command {
                        'm' => apply_sgr(&mut style, &parameters)?,
                        'K' if parameters.is_empty() || parameters == "0" => {
                            let cells = self.cells.get_mut(row).ok_or(FrameError::Bounds)?;
                            for cell in cells.iter_mut().skip(column) {
                                *cell = FrameCell {
                                    style: style.clone(),
                                    ..Default::default()
                                };
                            }
                        }
                        _ => {
                            return Err(FrameError::UnknownEscape(format!(
                                "CSI {parameters}{command}"
                            )));
                        }
                    }
                }
                '\r' => {
                    if characters.next() != Some('\n') {
                        return Err(FrameError::InvalidControl('\r'));
                    }
                    column = 0;
                    row += 1;
                    if row > self.height {
                        return Err(FrameError::Bounds);
                    }
                }
                control if control.is_control() => return Err(FrameError::InvalidControl(control)),
                printable => {
                    let mut text = String::from(printable);
                    while let Some(next) =
                        characters.peek().copied().filter(|next| !next.is_control())
                    {
                        text.push(next);
                        characters.next();
                    }
                    self.write_text(&text, &style, &mut column, row)?;
                }
            }
        }
        Ok(())
    }

    fn write_text(
        &mut self,
        text: &str,
        style: &FrameStyle,
        column: &mut usize,
        row: usize,
    ) -> Result<(), FrameError> {
        let mut prefix = String::new();
        let mut prefix_width = 0;
        let origin = *column;
        let mut last_lead: Option<usize> = None;
        for character in text.chars() {
            prefix.push(character);
            let next_width = UnicodeWidthStr::width(prefix.as_str());
            if UnicodeWidthChar::width(character).unwrap_or(0) == 0 || next_width <= prefix_width {
                let lead = last_lead.ok_or(FrameError::Bounds)?;
                let cell = self
                    .cells
                    .get_mut(row)
                    .and_then(|row| row.get_mut(lead))
                    .ok_or(FrameError::Bounds)?;
                cell.text.push(character);
                let span = UnicodeWidthStr::width(cell.text.as_str());
                for continuation in (lead + 1)..(lead + span) {
                    let cell = self
                        .cells
                        .get_mut(row)
                        .and_then(|row| row.get_mut(continuation))
                        .ok_or(FrameError::Bounds)?;
                    *cell = FrameCell {
                        continuation: true,
                        style: style.clone(),
                        ..Default::default()
                    };
                }
            } else {
                let span = next_width - prefix_width;
                let lead = origin + prefix_width;
                let cell = self
                    .cells
                    .get_mut(row)
                    .and_then(|row| row.get_mut(lead))
                    .ok_or(FrameError::Bounds)?;
                *cell = FrameCell {
                    text: character.to_string(),
                    style: style.clone(),
                    continuation: false,
                };
                for continuation in (lead + 1)..(lead + span) {
                    let cell = self
                        .cells
                        .get_mut(row)
                        .and_then(|row| row.get_mut(continuation))
                        .ok_or(FrameError::Bounds)?;
                    *cell = FrameCell {
                        continuation: true,
                        style: style.clone(),
                        ..Default::default()
                    };
                }
                last_lead = Some(lead);
            }
            prefix_width = next_width;
            *column = origin + next_width;
            if *column > self.width {
                return Err(FrameError::Bounds);
            }
        }
        Ok(())
    }
}

fn apply_sgr(style: &mut FrameStyle, parameters: &str) -> Result<(), FrameError> {
    let values = if parameters.is_empty() {
        vec![0]
    } else {
        parameters
            .split(';')
            .map(str::parse::<u16>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| FrameError::InvalidSgr(parameters.into()))?
    };
    let mut values = values.into_iter();
    while let Some(code) = values.next() {
        match code {
            0 => *style = FrameStyle::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            7 => style.invert = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            27 => style.invert = false,
            39 => style.foreground = None,
            49 => style.background = None,
            30..=37 | 90..=97 => {
                style.foreground = Some(Color::AnsiValue(
                    u8::try_from(if code < 90 { code - 30 } else { code - 90 + 8 })
                        .map_err(|_| FrameError::InvalidSgr(parameters.into()))?,
                ))
            }
            40..=47 | 100..=107 => {
                style.background = Some(Color::AnsiValue(
                    u8::try_from(if code < 100 {
                        code - 40
                    } else {
                        code - 100 + 8
                    })
                    .map_err(|_| FrameError::InvalidSgr(parameters.into()))?,
                ))
            }
            38 | 48 => {
                let mut channel = || {
                    values
                        .next()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| FrameError::InvalidSgr(parameters.into()))
                };
                let color = match channel()? {
                    5 => Color::AnsiValue(channel()?),
                    2 => Color::Rgb {
                        r: channel()?,
                        g: channel()?,
                        b: channel()?,
                    },
                    _ => return Err(FrameError::InvalidSgr(parameters.into())),
                };
                if code == 38 {
                    style.foreground = Some(color);
                } else {
                    style.background = Some(color);
                }
            }
            _ => return Err(FrameError::InvalidSgr(parameters.into())),
        }
    }
    Ok(())
}

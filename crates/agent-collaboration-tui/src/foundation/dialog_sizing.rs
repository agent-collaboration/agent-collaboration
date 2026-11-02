//! Validated per-axis sizing; physical space always wins over preferred minima.
use iocraft::prelude::Size;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogPercentage(u8);

impl DialogPercentage {
    pub const fn new(value: u8) -> Option<Self> {
        if value >= 1 && value <= 100 {
            Some(Self(value))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogAxisBounds {
    minimum: u16,
    maximum: u16,
}

impl DialogAxisBounds {
    pub const fn new(minimum: u16, maximum: u16) -> Option<Self> {
        if minimum <= maximum {
            Some(Self { minimum, maximum })
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogPreferredSize {
    Cells(u16),
    Percent(DialogPercentage),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogAxisSize {
    Content {
        maximum: Option<u16>,
    },
    Preferred {
        preferred: DialogPreferredSize,
        bounds: DialogAxisBounds,
    },
}

impl DialogAxisSize {
    pub const fn cells(cells: u16) -> Self {
        Self::Preferred {
            preferred: DialogPreferredSize::Cells(cells),
            bounds: DialogAxisBounds {
                minimum: cells,
                maximum: cells,
            },
        }
    }

    pub fn resolve(self, available: u16) -> (Size, u16) {
        match self {
            Self::Content { maximum } => (Size::Auto, maximum.unwrap_or(available).min(available)),
            Self::Preferred { preferred, bounds } => {
                let preferred = match preferred {
                    DialogPreferredSize::Cells(cells) => cells,
                    DialogPreferredSize::Percent(percent) => {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "DialogPercentage is at most 100, so the scaled size cannot exceed available u16 cells."
                        )]
                        let cells = (u32::from(available) * u32::from(percent.0) / 100) as u16;
                        cells
                    }
                };
                let resolved = preferred
                    .clamp(bounds.minimum, bounds.maximum)
                    .min(available);
                (Size::Length(u32::from(resolved)), resolved)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogPresentation {
    FullScreen,
    Sized {
        width: DialogAxisSize,
        height: DialogAxisSize,
    },
}

impl Default for DialogPresentation {
    fn default() -> Self {
        Self::Sized {
            width: DialogAxisSize::Preferred {
                preferred: DialogPreferredSize::Percent(DialogPercentage(80)),
                bounds: DialogAxisBounds {
                    minimum: 42,
                    maximum: 88,
                },
            },
            height: DialogAxisSize::Preferred {
                preferred: DialogPreferredSize::Percent(DialogPercentage(60)),
                bounds: DialogAxisBounds {
                    minimum: 8,
                    maximum: 24,
                },
            },
        }
    }
}

impl DialogPresentation {
    pub const fn cells(width: u16, height: u16) -> Self {
        Self::Sized {
            width: DialogAxisSize::cells(width),
            height: DialogAxisSize::cells(height),
        }
    }

    pub fn resolve(self, width: u16, height: u16) -> ((Size, u16), (Size, u16)) {
        match self {
            Self::FullScreen => (
                (Size::Length(u32::from(width)), width),
                (Size::Length(u32::from(height)), height),
            ),
            Self::Sized {
                width: width_policy,
                height: height_policy,
            } => (width_policy.resolve(width), height_policy.resolve(height)),
        }
    }
}

/// Vertical values are terminal rows; horizontal values are terminal columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DialogMargins {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

#[cfg(test)]
#[path = "dialog_sizing_test.rs"]
mod tests;

//! Quota window emphasis belongs to the account presentation, not generic controls.
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaTextRole {
    WeeklyWindow,
    ShortWindow,
}

impl QuotaTextRole {
    pub(crate) const fn color(self) -> Color {
        match self {
            Self::WeeklyWindow => Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
            Self::ShortWindow => Color::Rgb {
                r: 206,
                g: 212,
                b: 221,
            },
        }
    }
}

#[derive(Props)]
pub(crate) struct QuotaTextProps {
    pub content: String,
    #[iocraft(required)]
    pub role: QuotaTextRole,
    pub wrap: Option<TextWrap>,
}

#[component]
pub(crate) fn QuotaText(props: &QuotaTextProps) -> impl Into<AnyElement<'static>> {
    element!(Text(content:props.content.clone(),color:props.role.color(),weight:Weight::Normal,
        wrap:props.wrap.unwrap_or(TextWrap::NoWrap)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_window_roles_follow_the_adopted_palette() {
        for (role, expected) in [
            (
                QuotaTextRole::WeeklyWindow,
                Color::Rgb {
                    r: 237,
                    g: 240,
                    b: 245,
                },
            ),
            (
                QuotaTextRole::ShortWindow,
                Color::Rgb {
                    r: 206,
                    g: 212,
                    b: 221,
                },
            ),
        ] {
            let canvas = element!(QuotaText(content:"92% · week",role)).render(None);
            assert_eq!(
                canvas.cell(0, 0).unwrap().text_style().unwrap().color,
                Some(expected)
            );
        }
    }
}

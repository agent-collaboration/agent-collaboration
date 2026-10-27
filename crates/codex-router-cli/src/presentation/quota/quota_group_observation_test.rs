//! Test-only account/group attribution from mounted cells, never whole-page text.
use iocraft::Canvas;
use iocraft::prelude::Color;

#[derive(Clone, Copy, Default)]
enum ObservedGroup {
    #[default]
    Account,
    Quota,
    Pace,
    Extra,
}

#[derive(Default)]
pub(super) struct AlphaGroupObservation {
    witnessed_alpha: bool,
    owner_is_alpha: bool,
    group: ObservedGroup,
    pub(super) account: String,
    pub(super) quota: String,
    pub(super) pace: String,
    pub(super) extra: String,
}
impl AlphaGroupObservation {
    pub(super) fn observe(&mut self, canvas: &Canvas) {
        let text = canvas.to_string();
        let lines = text.lines().collect::<Vec<_>>();
        let wide_headers = lines.iter().find(|line| {
            line.contains("Account")
                && line.contains("Quota left")
                && line.contains("Weekly pace")
                && line.contains("Extra")
        });
        let wide_origins = wide_headers.map(|line| {
            ["Account", "Quota left", "Weekly pace", "Extra"].map(|label| {
                unicode_width::UnicodeWidthStr::width(line.split_once(label).unwrap().0)
            })
        });
        for (row, line) in lines.into_iter().enumerate() {
            // Literal adopted selected surface, rather than the production palette helper.
            let selected = (0..canvas.width()).any(|column| {
                canvas.cell(column, row).is_some_and(|cell| {
                    cell.background_color
                        == Some(Color::Rgb {
                            r: 52,
                            g: 59,
                            b: 70,
                        })
                })
            });
            if !selected {
                continue;
            }
            let words = line
                .replace(['│', '┃', '❯'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if words.contains("東京") {
                self.witnessed_alpha = true;
                self.owner_is_alpha = true;
            }
            if words.contains("Claude Research") {
                self.owner_is_alpha = false;
            }
            if !self.witnessed_alpha || !self.owner_is_alpha {
                continue;
            }
            if let Some(origins) = wide_origins {
                for (index, group) in [
                    ObservedGroup::Account,
                    ObservedGroup::Quota,
                    ObservedGroup::Pace,
                    ObservedGroup::Extra,
                ]
                .into_iter()
                .enumerate()
                {
                    let start = origins[index];
                    let end = origins.get(index + 1).copied().unwrap_or(canvas.width());
                    self.append(group, &cell_columns(line, start, end));
                }
            } else {
                match words.as_str() {
                    "Account" => self.group = ObservedGroup::Account,
                    "Quota left · reset in" => self.group = ObservedGroup::Quota,
                    "Weekly pace" => self.group = ObservedGroup::Pace,
                    "Extra" => self.group = ObservedGroup::Extra,
                    _ => self.append(self.group, &words),
                }
            }
        }
    }
    fn append(&mut self, group: ObservedGroup, words: &str) {
        let destination = match group {
            ObservedGroup::Account => &mut self.account,
            ObservedGroup::Quota => &mut self.quota,
            ObservedGroup::Pace => &mut self.pace,
            ObservedGroup::Extra => &mut self.extra,
        };
        destination.push(' ');
        destination.push_str(words);
    }
}
fn cell_columns(line: &str, start: usize, end: usize) -> String {
    let mut column = 0;
    line.chars()
        .filter(|character| {
            let previous = column;
            column += unicode_width::UnicodeWidthChar::width(*character).unwrap_or(0);
            previous >= start && previous < end && !matches!(character, '│' | '┃' | '─')
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn alpha_oracle_rejects_beta_selection_and_quota_value_moved_into_extra() {
    use iocraft::prelude::*;
    // Both counterexamples carry the wanted quota words, but at the wrong owner/group.
    for (name, group) in [
        (
            "Claude Research Account Beta Full Name",
            "Quota left · reset in",
        ),
        ("東京 Alpha", "Extra"),
    ] {
        let canvas=element!(View(width:50,flex_direction:FlexDirection::Column,background_color:Color::Rgb {r:52,g:59,b:70}) {
            Text(content:name) Text(content:group) Text(content:"99% · 5h · 5h 0m")
        }).render(None);
        let mut observed = AlphaGroupObservation::default();
        observed.observe(&canvas);
        assert!(
            !observed.quota.contains("99%"),
            "wrong owner/group cannot satisfy Alpha quota"
        );
    }
    let canvas=element!(View(width:50,flex_direction:FlexDirection::Column,background_color:Color::Rgb {r:52,g:59,b:70}) {
        Text(content:"東京 Alpha") Text(content:"Quota left · reset in") Text(content:"99% · 5h · 5h 0m")
    }).render(None);
    let mut observed = AlphaGroupObservation::default();
    observed.observe(&canvas);
    assert!(observed.quota.contains("99% · 5h · 5h 0m"));
}

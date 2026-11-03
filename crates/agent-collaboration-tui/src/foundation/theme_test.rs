use agent_collaboration_tui::{
    UiSurface, UiTextRole,
    foundation::theme::{SurfaceRole, TextRole, Theme, UiGlyph},
};
#[test]
fn theme_keeps_shipped_colors_and_dims_every_role_and_background() {
    let normal = Theme::default();
    let covered = Theme::covered();
    assert_eq!(normal.foreground(TextRole::Text), UiTextRole::Body.color());
    assert_eq!(
        normal.foreground(TextRole::Focused),
        UiTextRole::Selected.color()
    );
    assert_eq!(
        normal.background(SurfaceRole::Base),
        UiSurface::Base.color()
    );
    assert_eq!(
        normal.background(SurfaceRole::GroupPanel),
        UiSurface::Raised.color()
    );
    for role in [
        TextRole::Text,
        TextRole::Muted,
        TextRole::Accent,
        TextRole::Focused,
        TextRole::Primary,
        TextRole::Disabled,
        TextRole::Dimmed,
        TextRole::Good,
        TextRole::Warning,
        TextRole::Error,
    ] {
        assert_ne!(normal.foreground(role), covered.foreground(role));
    }
    for role in [
        SurfaceRole::Base,
        SurfaceRole::Raised,
        SurfaceRole::GroupPanel,
        SurfaceRole::Header,
        SurfaceRole::Selected,
        SurfaceRole::Alternate,
    ] {
        assert_ne!(normal.background(role), covered.background(role));
    }
}

#[test]
fn every_owned_glyph_is_one_terminal_cell_in_both_width_tables() {
    for glyph in UiGlyph::ALL {
        assert_eq!(
            unicode_width_legacy::UnicodeWidthStr::width(glyph.text()),
            1,
            "legacy: {glyph:?}"
        );
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(glyph.text()),
            1,
            "current: {glyph:?}"
        );
    }
}

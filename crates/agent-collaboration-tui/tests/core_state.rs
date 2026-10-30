#[cfg(test)]
mod tests {
    use agent_collaboration_tui::{
        UiSurface, UiTextRole,
        actions::{
            ActionDef, ActionError, ActionId, ActionRegistry, Availability, AvailabilityFn,
            KeyChord, Placement, ScopeId,
        },
        focus_model::{FocusDirection, FocusModel, GroupId, TargetId, TargetSpec},
        surface_stack::{OverlayKind, OverlaySpec, SurfaceId, SurfaceModel, SurfaceStack},
        theme::{SurfaceRole, TextRole, Theme, UiGlyph},
    };
    use iocraft::{Handler, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    fn target(name: &str) -> TargetSpec {
        TargetSpec::new(TargetId::new(name).unwrap())
    }
    fn action(name: &str, chord: &str) -> ActionDef {
        ActionDef {
            toolbar_label: None,
            id: ActionId::new(name).unwrap(),
            title: "Open".into(),
            keys: vec![chord.parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::default(),
        }
    }
    fn surface(names: &[&str]) -> SurfaceModel {
        SurfaceModel {
            focus: FocusModel::new(names.iter().map(|name| target(name)).collect()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("screen").unwrap(), vec![]).unwrap(),
            primary_action: None,
        }
    }

    #[test]
    fn identities_reject_absence_without_collapsing_distinct_domains() {
        assert!(TargetId::new(" ").is_err());
        assert!(ActionId::new("").is_err());
        assert!(ScopeId::new("\n").is_err());
        assert!(GroupId::new("").is_err());
        assert_ne!(
            TargetId::new("endpoint-a/session").unwrap(),
            TargetId::new("endpoint-b/session").unwrap()
        );
    }

    #[test]
    fn raw_unit_separator_normalizes_to_the_printable_control_seven_chord() {
        let printable: KeyChord = "Ctrl+7".parse().unwrap();
        let raw = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('\u{1f}'));
        assert!(
            printable.matches(&raw),
            "raw US must match the printable binding"
        );
        let raw_binding: KeyChord = "\u{1f}".parse().unwrap();
        assert_eq!(raw_binding.to_string(), "Ctrl+7");
        assert!(!raw_binding.to_string().chars().any(char::is_control));
        assert_eq!(raw_binding, printable);
    }
    #[test]
    fn duplicate_key_in_one_scope_is_rejected_including_equivalent_spellings() {
        for (first, second) in [
            ("Ctrl+N", "control+n"),
            ("Shift+Tab", "backtab"),
            ("F10", "f10"),
        ] {
            let result = ActionRegistry::new(
                ScopeId::new("screen").unwrap(),
                vec![action("one", first), action("two", second)],
            );
            assert!(
                matches!(result, Err(ActionError::DuplicateKey(_))),
                "{first}, {second}: {result:?}"
            );
        }
        assert!(
            ActionRegistry::new(
                ScopeId::new("screen").unwrap(),
                vec![action("one", "F1"), action("one", "F2")]
            )
            .is_err()
        );
        // The same chord in distinct scopes is intentionally legal.
        for scope in ["screen", "dialog"] {
            assert!(
                ActionRegistry::new(ScopeId::new(scope).unwrap(), vec![action("open", "Enter")])
                    .is_ok()
            );
        }
    }

    #[test]
    fn key_parser_matches_aliases_but_never_repeat_or_release_effects() {
        for (text, code, modifiers, display) in [
            (
                "control+shift+enter",
                KeyCode::Enter,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                "Ctrl+Shift+Enter",
            ),
            ("option+f5", KeyCode::F(5), KeyModifiers::ALT, "Alt+F5"),
            ("cmd+k", KeyCode::Char('K'), KeyModifiers::SUPER, "Super+K"),
            (
                "backtab",
                KeyCode::BackTab,
                KeyModifiers::SHIFT,
                "Shift+Tab",
            ),
        ] {
            let chord: KeyChord = text.parse().unwrap();
            assert_eq!(chord.to_string(), display);
            for kind in [
                KeyEventKind::Press,
                KeyEventKind::Repeat,
                KeyEventKind::Release,
            ] {
                let mut event = KeyEvent::new(kind, code);
                event.modifiers = modifiers;
                assert_eq!(chord.matches(&event), kind == KeyEventKind::Press);
            }
        }
        for invalid in ["", "ctrl", "a+b", "f25", "ctrl+unknown"] {
            assert!(invalid.parse::<KeyChord>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn action_availability_is_read_for_every_invocation() {
        let available = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut definition = action("start", "Ctrl+Enter");
        definition.availability = AvailabilityFn::new({
            let available = available.clone();
            move || {
                if available.load(Ordering::SeqCst) {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "already started".into(),
                    }
                }
            }
        });
        definition.run = Handler::from({
            let calls = calls.clone();
            move |()| {
                calls.fetch_add(1, Ordering::SeqCst);
                available.store(false, Ordering::SeqCst);
            }
        });
        let registry = ActionRegistry::new(ScopeId::new("new").unwrap(), vec![definition]).unwrap();
        for _ in 0..2 {
            if let Some(handler) = registry.invocation(&ActionId::new("start").unwrap()) {
                handler(());
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            registry.presentations()[0].availability,
            Availability::Unavailable {
                reason: "already started".into()
            }
        );
    }

    #[test]
    fn focus_traversal_wraps_over_every_enabled_target_and_only_within_its_group() {
        for length in 0..=5 {
            for enabled_mask in 0..(1usize << length) {
                let targets = (0..length)
                    .map(|index| {
                        let enabled = enabled_mask & (1 << index) != 0;
                        target(&format!("row-{index}"))
                            .in_group(GroupId::new(format!("group-{}", index % 2)).unwrap())
                            .with_availability(AvailabilityFn::new(move || {
                                if enabled {
                                    Availability::Available
                                } else {
                                    Availability::Unavailable {
                                        reason: "disabled".into(),
                                    }
                                }
                            }))
                    })
                    .collect::<Vec<_>>();
                let enabled = targets
                    .iter()
                    .filter(|target| target.is_enabled())
                    .map(|target| target.id.clone())
                    .collect::<Vec<_>>();
                let mut focus = FocusModel::new(targets).unwrap();
                assert_eq!(focus.current(), enabled.first());
                if enabled.is_empty() {
                    focus.traverse(FocusDirection::Next);
                    assert!(focus.current().is_none());
                    continue;
                }
                for expected in enabled.iter().cycle().skip(1).take(enabled.len() * 2) {
                    focus.traverse(FocusDirection::Next);
                    assert_eq!(focus.current(), Some(expected));
                }
                for expected in enabled.iter().rev().cycle().take(enabled.len() * 2) {
                    focus.traverse(FocusDirection::Previous);
                    assert_eq!(focus.current(), Some(expected));
                }
                for starting in &enabled {
                    focus.focus(starting);
                    let group = focus.focused().unwrap().group.clone();
                    let grouped = focus
                        .targets()
                        .iter()
                        .filter(|target| target.is_enabled() && target.group == group)
                        .map(|target| target.id.clone())
                        .collect::<Vec<_>>();
                    let position = grouped.iter().position(|id| id == starting).unwrap();
                    for expected in grouped
                        .iter()
                        .cycle()
                        .skip(position + 1)
                        .take(grouped.len() * 2)
                    {
                        focus.traverse_group(FocusDirection::Next);
                        assert_eq!(focus.current(), Some(expected));
                    }
                    focus.traverse_group(FocusDirection::Previous);
                    focus.traverse_group(FocusDirection::Next);
                    assert_eq!(focus.current(), Some(starting));
                }
            }
        }
        assert!(FocusModel::new(vec![target("same"), target("same")]).is_err());
    }

    #[test]
    fn closing_restores_the_captured_identity_through_reordering_and_falls_back_if_gone() {
        for replacement in [vec!["beta", "alpha"], vec!["beta"], vec![]] {
            let mut stack = SurfaceStack::new(surface(&["alpha", "beta"]));
            let first_id = stack
                .open_overlay(OverlaySpec {
                    view: Default::default(),
                    layout: Default::default(),
                    kind: OverlayKind::Menu,
                    model: surface(&["menu-item"]),
                })
                .unwrap();
            assert!(stack.is_covered(SurfaceId::Screen));
            stack
                .surface_mut(SurfaceId::Screen)
                .unwrap()
                .focus
                .replace_targets(replacement.iter().map(|name| target(name)).collect())
                .unwrap();
            stack.close_top();
            let expected = if replacement.contains(&"alpha") {
                Some("alpha")
            } else {
                replacement.first().copied()
            };
            assert_eq!(stack.top().focus.current().map(TargetId::as_str), expected);
            assert_eq!(stack.top_id(), SurfaceId::Screen);
            assert!(!stack.is_covered(SurfaceId::Screen));
            let second_id = stack
                .open_overlay(OverlaySpec {
                    view: Default::default(),
                    layout: Default::default(),
                    kind: OverlayKind::Dialog,
                    model: surface(&["close"]),
                })
                .unwrap();
            assert_ne!(first_id, second_id);
        }
        let mut stack = SurfaceStack::new(surface(&["alpha", "beta"]));
        stack
            .open_overlay(OverlaySpec {
                view: Default::default(),
                layout: Default::default(),
                kind: OverlayKind::Menu,
                model: surface(&["item"]),
            })
            .unwrap();
        let disabled =
            target("alpha").with_availability(AvailabilityFn::new(|| Availability::Unavailable {
                reason: "gone".into(),
            }));
        stack
            .surface_mut(SurfaceId::Screen)
            .unwrap()
            .focus
            .replace_targets(vec![disabled, target("beta")])
            .unwrap();
        stack.close_top();
        assert_eq!(stack.top().focus.current().unwrap().as_str(), "beta");
    }

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
}

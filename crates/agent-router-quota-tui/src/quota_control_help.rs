//! Consumer context accompanies Help's live action projection; it adds no fake actions.
use super::quota_control_context::{QuotaControlContext, QuotaDialogKind};
use agent_collaboration_tui::dialog::help::{HelpContent, HelpContextLine, HelpInputConvention};

pub(crate) fn quota_header_help_context(context: &QuotaControlContext) -> Vec<HelpContextLine> {
    let model = context.view_model.read();
    let mut lines = vec![
        HelpContextLine {
            label: "Pool".into(),
            value: model.header.pool.summary(usize::MAX),
        },
        HelpContextLine {
            label: "Data".into(),
            value: model
                .header
                .freshness
                .summary(model.header.observed_clients),
        },
    ];
    if let Some(notice) = &model.header.credential_notice {
        lines.push(HelpContextLine {
            label: "Credentials".into(),
            value: notice.clone(),
        });
    }
    lines
}

pub(crate) fn quota_help_content(
    context: &QuotaControlContext,
    kind: &QuotaDialogKind,
) -> HelpContent {
    let mut content = HelpContent {
        context: Vec::new(),
        ..HelpContent::default()
    };
    match kind {
        QuotaDialogKind::Account(target) => {
            content.context.push(HelpContextLine {
                label: "Account".into(),
                value: target.label.as_str().to_owned(),
            });
            content.context.push(HelpContextLine {
                label: "Account ID".into(),
                value: target.account_id.as_str().to_owned(),
            });
            content.context.push(HelpContextLine {
                label: "Provider".into(),
                value: target.provider.as_str().to_owned(),
            });
        }
        QuotaDialogKind::Options => {
            if let Some(options) = context.account_options.read().as_ref() {
                content.context.push(HelpContextLine {
                    label: "Account".into(),
                    value: format!(
                        "{} [{}]",
                        options.target.account_label, options.target.account_tag
                    ),
                });
                content.context.push(HelpContextLine {
                    label: "Account ID".into(),
                    value: options.target.account_id.as_str().to_owned(),
                });
            }
        }
        QuotaDialogKind::Floor => {
            if let Some(editor) = context.weekly_floor_editor.read().as_ref() {
                content.context.push(HelpContextLine {
                    label: "Account".into(),
                    value: editor.account_label.clone(),
                });
                content.context.push(HelpContextLine {
                    label: "Account ID".into(),
                    value: editor.account_id.as_str().to_owned(),
                });
            }
        }
        QuotaDialogKind::Pool => content.context.push(HelpContextLine {
            label: "Scope".into(),
            value: "Pool-wide quota facts".into(),
        }),
        QuotaDialogKind::Filters => content.context.push(HelpContextLine {
            label: "Scope".into(),
            value: "Account filters; changes apply only when committed".into(),
        }),
    }
    content.context.push(HelpContextLine {
        label: "Screen".into(),
        value: match kind {
            QuotaDialogKind::Account(_) => "Account details",
            QuotaDialogKind::Pool => "Pool details",
            QuotaDialogKind::Filters => "Account filters",
            QuotaDialogKind::Floor => "Weekly floor",
            QuotaDialogKind::Options => "Account options",
        }
        .into(),
    });
    content.context.extend(quota_header_help_context(context));
    if matches!(kind, QuotaDialogKind::Options) {
        for (label, value) in [
            (
                "Included quota",
                "Included quota on eligible accounts always comes first.",
            ),
            (
                "Allow credits",
                "Allow is limited to eligible OpenAI Responses requests.",
            ),
            (
                "Inspect",
                "Inspect reset eligibility. No reset is consumed without explicit confirmation.",
            ),
        ] {
            content.context.push(HelpContextLine {
                label: label.into(),
                value: value.into(),
            });
        }
    }
    content.conventions = vec![
        HelpInputConvention {
            key: "Tab/Shift+Tab".into(),
            description: "Move through enabled controls".into(),
        },
        HelpInputConvention {
            key: "↑ / ↓ / ← / →".into(),
            description: "Move within a control group".into(),
        },
        HelpInputConvention {
            key: "Enter".into(),
            description: "Activate the focused control".into(),
        },
        HelpInputConvention {
            key: "Ctrl+Enter".into(),
            description: "Run the marked primary action".into(),
        },
        HelpInputConvention {
            key: "Esc".into(),
            description: if matches!(kind, QuotaDialogKind::Account(_) | QuotaDialogKind::Pool) {
                "Close details"
            } else {
                "Close one menu or dialog; never quit quota"
            }
            .into(),
        },
        HelpInputConvention {
            key: "PgUp/PgDn · Option+↑/↓".into(),
            description: "Page".into(),
        },
        HelpInputConvention {
            key: "Home/End · Option+Shift+↑/↓".into(),
            description: "First/last".into(),
        },
        HelpInputConvention {
            key: "Wheel".into(),
            description: "Scroll the current content".into(),
        },
    ];
    if matches!(kind, QuotaDialogKind::Filters | QuotaDialogKind::Options) {
        content.conventions.push(HelpInputConvention {
            key: "Type / paste".into(),
            description: "Filter choices; Enter picks and Esc keeps the field unchanged".into(),
        });
    }
    content.conventions.push(HelpInputConvention {
        key: "Ctrl+C".into(),
        description: "Exit Quota unless a reset is being applied.".into(),
    });
    content
}

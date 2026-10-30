//! Shared field columns and an always-reserved row selector gutter.
use crate::{
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, Theme, UiGlyph},
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

/// Typed rows keep their labels available to the container; section content is unlabelled.
pub enum FormFieldsChild {
    Field(Element<'static, FormField>),
    Content(AnyElement<'static>),
}

impl From<Element<'static, FormField>> for FormFieldsChild {
    fn from(field: Element<'static, FormField>) -> Self {
        Self::Field(field)
    }
}

impl From<AnyElement<'static>> for FormFieldsChild {
    fn from(content: AnyElement<'static>) -> Self {
        Self::Content(content)
    }
}

#[derive(Props)]
pub struct FormFieldsProps {
    pub stacked: bool,
    pub row_gap: u16,
    pub children: Vec<FormFieldsChild>,
}

struct FormFieldColumns {
    label_width: u32,
    stacked: bool,
}

/// One measured label column and one reserved selector gutter for every row.
#[component]
pub fn FormFields(props: &mut FormFieldsProps) -> impl Into<AnyElement<'static>> {
    let label_width = props
        .children
        .iter()
        .filter_map(|child| match child {
            FormFieldsChild::Field(field) => {
                Some(UnicodeWidthStr::width(field.props.label.as_str()))
            }
            FormFieldsChild::Content(_) => None,
        })
        .max()
        .unwrap_or_default();
    let columns = FormFieldColumns {
        label_width: u32::try_from(label_width).unwrap_or(u32::MAX),
        stacked: props.stacked,
    };
    let children = std::mem::take(&mut props.children)
        .into_iter()
        .map(|child| match child {
            FormFieldsChild::Field(field) => field.into_any(),
            FormFieldsChild::Content(content) => content,
        });
    element! {
        ContextProvider(value:Context::owned(columns)) {
            View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:props.row_gap) {#(children)}
        }
    }
}

#[derive(Props)]
pub struct FormFieldProps<'a> {
    pub label: String,
    pub has_focus: bool,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn FormField<'a>(props: &mut FormFieldProps<'a>, hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let columns = hooks.use_context::<FormFieldColumns>();
    let theme = hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |context| context.theme());
    let children = std::mem::take(&mut props.children);
    element! {
        View(width:100pct,flex_direction: if columns.stacked { FlexDirection::Column } else { FlexDirection::Row }, column_gap: 2, flex_shrink: 0.0_f32) {
            View(width: if columns.stacked { Size::Auto } else { Size::Length(columns.label_width) }, flex_shrink: 0.0_f32) {
                Text(content: props.label.clone(), color: theme.foreground(TextRole::Accent))
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0_f32, flex_shrink:1.0_f32,min_width: 0,max_width:100pct) {
                SelectableRow(selected:props.has_focus) { #(children) }
            }
        }
    }
}

#[derive(Props)]
pub struct SelectableRowProps<'a> {
    pub selected: bool,
    pub surface: Option<SurfaceRole>,
    pub hovered: bool,
    pub marker_offset: u16,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn SelectableRow<'a>(
    props: &mut SelectableRowProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    super::scroll_list::use_scroll_reveal(&mut hooks, props.selected);
    let theme = hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |context| context.theme());
    let background = if props.selected {
        Some(SurfaceRole::Selected)
    } else if props.hovered && props.surface.is_some() {
        Some(SurfaceRole::Raised)
    } else {
        props.surface
    };
    let children = std::mem::take(&mut props.children);
    element! {
        View(background_color:background.map(|role|theme.background(role)),flex_shrink: 0.0_f32) {
            View(width: 2, flex_shrink: 0.0_f32,flex_direction:FlexDirection::Column,padding_top:props.marker_offset) {
                Text(content: if props.selected { UiGlyph::Focus.text() } else { "" }, color: theme.foreground(TextRole::Focused))
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0_f32, flex_shrink:1.0_f32,min_width: 0,max_width:100pct) { #(children) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_unicode_labels_and_focus_share_one_value_origin() {
        for stacked in [false, true] {
            for selected in [false, true] {
                let frame = element!(View(width:40) {
                    FormFields(stacked) {
                        FormField(label:"マシン設定".to_owned(),has_focus:selected) {Text(content:"Sunbook")}
                        FormField(label:"Folder".to_owned()) {Text(content:"/repo")}
                    }
                }).render(None).to_string();
                for value in ["Sunbook", "/repo"] {
                    let prefix = frame
                        .lines()
                        .find(|line| line.contains(value))
                        .unwrap()
                        .split_once(value)
                        .unwrap()
                        .0;
                    assert_eq!(
                        UnicodeWidthStr::width(prefix),
                        if stacked { 2 } else { 14 },
                        "{frame}"
                    );
                }
                assert_eq!(frame.matches('❯').count(), usize::from(selected), "{frame}");
            }
        }
    }
}

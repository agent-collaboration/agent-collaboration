//! Shared field columns and an always-reserved row selector gutter.
use crate::{
    foundation::theme::{SurfaceRole, TextRole, Theme, UiGlyph},
    surfaces::surface_handle::SurfaceContext,
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
    pub minimum_label_width: Option<u32>,
    pub children: Vec<FormFieldsChild>,
}

struct FormFieldColumns {
    label_width: u32,
    stacked: bool,
}

/// One measured label column and one reserved selector gutter for every row.
#[component]
pub fn FormFields(props: &mut FormFieldsProps) -> impl Into<AnyElement<'static>> {
    let labels = props.children.iter().filter_map(|child| match child {
        FormFieldsChild::Field(field) => Some(field.props.label.as_str()),
        FormFieldsChild::Content(_) => None,
    });
    let label_width =
        FormFields::label_width_for(labels).max(props.minimum_label_width.unwrap_or_default());
    let columns = FormFieldColumns {
        label_width,
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

impl FormFields {
    /// Sections can share a display-width budget without duplicating measurement rules.
    pub fn label_width_for<TLabel: AsRef<str>>(labels: impl IntoIterator<Item = TLabel>) -> u32 {
        let width = labels
            .into_iter()
            .map(|label| UnicodeWidthStr::width(label.as_ref()))
            .max()
            .unwrap_or_default();
        u32::try_from(width).unwrap_or(u32::MAX)
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
        View(width:100pct,flex_direction: if columns.stacked { FlexDirection::Column } else { FlexDirection::Row }, column_gap: 2, flex_wrap:FlexWrap::Wrap, flex_shrink: 0.0_f32) {
            View(width: if columns.stacked { Size::Auto } else { Size::Length(columns.label_width) }, max_width:100pct, flex_shrink: 0.0_f32) {
                Text(content: props.label.clone(), color: theme.foreground(TextRole::Primary))
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
    let target_id = hooks
        .try_use_context::<SurfaceContext>()
        .and_then(|context| {
            context
                .stack
                .snapshot()
                .surface(context.surface)
                .and_then(|surface| surface.focus.current().cloned())
        })
        .filter(|_| props.selected);
    crate::controls::list::scroll_list::use_scroll_reveal(&mut hooks, props.selected, target_id);
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
#[path = "field_reason_layout_test.rs"]
mod reason_layout_test;
#[cfg(test)]
#[path = "responsive_fields_test.rs"]
mod responsive_test;
#[cfg(test)]
#[path = "form_fields_test.rs"]
mod tests;

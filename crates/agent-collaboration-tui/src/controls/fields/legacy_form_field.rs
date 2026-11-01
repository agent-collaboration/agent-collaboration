//! Shared field columns and an always-reserved row selector gutter.
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
pub fn FormField<'a>(
    props: &mut FormFieldProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    crate::controls::focus_reveal::use_focus_reveal(&mut hooks, props.has_focus);
    let columns = hooks.use_context::<FormFieldColumns>();
    let children = std::mem::take(&mut props.children);
    element! {
        View(width:100pct,flex_direction: if columns.stacked { FlexDirection::Column } else { FlexDirection::Row }, column_gap: 2, flex_shrink: 0.0_f32) {
            View(width: if columns.stacked { Size::Auto } else { Size::Length(columns.label_width) }, flex_shrink: 0.0_f32) {
                Text(content: props.label.clone(), color: crate::UiTextRole::Label.color())
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
    pub surface: Option<crate::UiSurface>,
    pub hovered: bool,
    pub marker_offset: u16,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn SelectableRow<'a>(
    props: &mut SelectableRowProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    crate::controls::focus_reveal::use_focus_reveal(&mut hooks, props.selected);
    let background = if props.selected {
        Some(crate::UiSurface::Selected)
    } else if props.hovered && props.surface.is_some() {
        Some(crate::UiSurface::Raised)
    } else {
        props.surface
    };
    let children = std::mem::take(&mut props.children);
    element! {
        View(background_color:background.map(crate::UiSurface::color),flex_shrink: 0.0_f32) {
            View(width: 2, flex_shrink: 0.0_f32,flex_direction:FlexDirection::Column,padding_top:props.marker_offset) {
                Text(content: if props.selected { "❯" } else { "" }, color: Color::Yellow)
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0_f32, flex_shrink:1.0_f32,min_width: 0,max_width:100pct) { #(children) }
        }
    }
}

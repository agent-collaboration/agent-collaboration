//! Shared field columns and an always-reserved row selector gutter.
use iocraft::prelude::*;

#[derive(Props)]
pub struct FormFieldProps<'a> {
    pub label: String,
    pub label_width: u16,
    pub stacked: bool,
    pub has_focus: bool,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn FormField<'a>(
    props: &mut FormFieldProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    crate::dialog_focus_reveal::use_focus_reveal(&mut hooks, props.has_focus);
    let children = std::mem::take(&mut props.children);
    element! {
        View(width:100pct,flex_direction: if props.stacked { FlexDirection::Column } else { FlexDirection::Row }, column_gap: 2, flex_shrink: 0.0_f32) {
            View(width: if props.stacked { Size::Auto } else { Size::Length(u32::from(props.label_width)) }, flex_shrink: 0.0_f32) {
                Text(content: props.label.clone(), color: Color::White)
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0_f32, flex_shrink:1.0_f32,min_width: 0,max_width:100pct) { #(children) }
        }
    }
}

#[derive(Props)]
pub struct SelectableRowProps<'a> {
    pub selected: bool,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn SelectableRow<'a>(
    props: &mut SelectableRowProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    crate::dialog_focus_reveal::use_focus_reveal(&mut hooks, props.selected);
    let children = std::mem::take(&mut props.children);
    element! {
        View(flex_shrink: 0.0_f32) {
            View(width: 2, flex_shrink: 0.0_f32) {
                Text(content: if props.selected { "❯" } else { "" }, color: Color::Yellow)
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0_f32, flex_shrink:1.0_f32,min_width: 0,max_width:100pct) { #(children) }
        }
    }
}

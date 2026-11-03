use super::{AnchorScene, Bounds, bounds, popup_bounds, published_anchor};
use crate::{
    foundation::theme::{SurfaceRole, Theme},
    surfaces::{surface_handle::SurfaceStackHandle, surface_stack::SurfaceId},
    test_support::{
        event_driver::{EventDriver, InputMode},
        frame_grid::FrameGrid,
        grids::STANDARD_GRIDS,
        input_scene::Scene,
        list_scene::key,
        recording_backend::recording_terminal_render_loop,
    },
};
use futures_util::StreamExt;
use iocraft::{Canvas, prelude::*};
use std::sync::{Arc, Mutex};

fn click(column: u16, row: u16) -> TerminalEvent {
    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        column,
        row,
    ))
}
fn assert_gutter(frame: &FrameGrid, raw: &Canvas, rect: Bounds, label: &str) {
    let expected = Some(Theme::default().background(SurfaceRole::Raised));
    for row in rect.top..=rect.bottom {
        if rect.left > 0 {
            let gutter = frame
                .cell(usize::from(rect.left - 1), usize::from(row))
                .unwrap();
            if gutter.continuation || gutter.style.background != expected {
                eprintln!(
                    "GUTTER {label} row{row} rect={rect:?} raw_bg={:?} serialized={gutter:?} raw_left_neighbor={:?}",
                    raw.cell(usize::from(rect.left - 1), usize::from(row))
                        .unwrap()
                        .background_color,
                    rect.left
                        .checked_sub(2)
                        .and_then(|column| raw.cell(usize::from(column), usize::from(row)))
                        .and_then(|cell| cell.text())
                );
            }
            assert!(
                !gutter.continuation,
                "the painted popup gutter is a standalone cell: {label} row{row}"
            );
            assert_eq!(
                gutter.text, " ",
                "the popup gutter clears underlying text: {label} row{row}"
            );
            assert_eq!(
                gutter.style.background, expected,
                "the popup gutter uses the overlay background on every row: {label} row{row}"
            );
        }
        assert_eq!(
            frame
                .cell(usize::from(rect.left), usize::from(row))
                .unwrap()
                .text,
            if row == rect.top {
                "╭"
            } else if row == rect.bottom {
                "╰"
            } else {
                "│"
            },
            "{label}: complete left border row{row}"
        );
        assert_eq!(
            frame
                .cell(usize::from(rect.right), usize::from(row))
                .unwrap()
                .text,
            if row == rect.top {
                "╮"
            } else if row == rect.bottom {
                "╯"
            } else {
                "│"
            },
            "{label}: complete right border row{row}"
        );
    }
    for column in rect.left + 1..rect.right {
        for row in [rect.top, rect.bottom] {
            assert_eq!(
                frame
                    .cell(usize::from(column), usize::from(row))
                    .unwrap()
                    .text,
                "─",
                "{label}: complete horizontal border"
            );
        }
    }
}
fn published_popup_bounds(stack: SurfaceStackHandle) -> Bounds {
    let map = stack.hit_regions().published().unwrap();
    let surface = stack.snapshot().top_id();
    bounds(
        &map.surfaces
            .iter()
            .find(|region| region.surface == surface)
            .unwrap()
            .cells,
    )
}
fn assert_narrow_outside(frame: &FrameGrid, rect: Bounds, pattern: &str) {
    if pattern == "x" && rect.left >= 2 {
        let outside = frame
            .cell(usize::from(rect.left - 2), usize::from(rect.top))
            .unwrap();
        assert_eq!(
            outside.text, "x",
            "ordinary text outside the one-cell gutter stays present"
        );
        assert_eq!(
            outside.style.background,
            Some(Theme::covered().background(SurfaceRole::Base)),
            "ordinary background outside the one-cell gutter stays dimmed"
        );
    }
}
fn assert_anchored(
    frame: &FrameGrid,
    stack: SurfaceStackHandle,
    menu: bool,
    width: u16,
    height: u16,
    above: bool,
) -> Bounds {
    assert_anchor_bounds(popup_bounds(frame), stack, menu, width, height, above)
}
fn assert_anchor_bounds(
    rect: Bounds,
    stack: SurfaceStackHandle,
    menu: bool,
    width: u16,
    height: u16,
    above: bool,
) -> Bounds {
    let anchor = published_anchor(stack, menu);
    assert!(rect.right < width && rect.bottom < height);
    if above {
        assert_eq!(
            rect.bottom + 1,
            anchor.top,
            "pointer-opened popup flips directly above its control"
        );
    } else {
        assert_eq!(
            rect.top,
            anchor.bottom + 1,
            "pointer-opened popup sits directly below its control"
        );
    }
    assert_eq!(
        rect.left,
        anchor.left.min(width - (rect.right - rect.left + 1)),
        "pointer-opened popup clamps horizontally"
    );
    rect
}
#[tokio::test]
async fn pointer_opened_popups_anchor_above_below_and_clamp_at_standard_grids() {
    for (width, height) in STANDARD_GRIDS {
        for menu in [false, true] {
            for near_bottom in [false, true] {
                let (mut driver, config, journal) = EventDriver::new();
                let handle = Arc::new(Mutex::new(None));
                let raw = Arc::new(Mutex::new(Vec::new()));
                let captured = raw.clone();
                let mut ui = element!(AnchorScene(journal,width,height,menu,near_bottom,handle:handle.clone()));
                let mut frames = Box::pin(
                    recording_terminal_render_loop(&mut ui, config)
                        .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
                );
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let stack = handle.lock().unwrap().unwrap();
                    let anchor = published_anchor(stack, menu);
                    let start = raw.lock().unwrap().len();
                    driver
                        .drive(
                            &mut frames,
                            InputMode::FrameSeparated,
                            vec![(click(anchor.left, anchor.top), true)],
                            true,
                        )
                        .await;
                    assert!(
                        matches!(stack.snapshot().top_id(), SurfaceId::Overlay(_)),
                        "pointer press opens the actual control"
                    );
                    let first =
                        FrameGrid::from_canvas(raw.lock().unwrap().get(start).unwrap()).unwrap();
                    assert_anchored(&first, stack, menu, width, height, near_bottom);
                })
                .await
                .unwrap();
            }
        }
    }
}
#[tokio::test]
async fn anchored_gutters_cover_wide_spans_after_resize_close_and_reopen_at_standard_grids() {
    let mut leading = 0;
    let mut trailing = 0;
    let mut narrow = 0;
    let mut left_edge = 0;
    for (index, (width, height)) in STANDARD_GRIDS.into_iter().enumerate() {
        let (new_width, new_height) = STANDARD_GRIDS[(index + 1) % STANDARD_GRIDS.len()];
        for pattern in ["📂", "✉️", "界", "x"] {
            for menu in [false, true] {
                for (offset, at_left) in [(0u16, false), (1u16, false), (0u16, true)] {
                    let (mut driver, config, journal) = EventDriver::new();
                    let handle = Arc::new(Mutex::new(None));
                    let raw = Arc::new(Mutex::new(Vec::new()));
                    let captured = raw.clone();
                    let mut ui = element!(AnchorScene(journal,width,height,menu,anchor_at_left:at_left,underlay_pattern:pattern.to_owned(),underlay_offset:offset,handle:handle.clone()));
                    let mut frames = Box::pin(
                        recording_terminal_render_loop(&mut ui, config)
                            .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
                    );
                    tokio::time::timeout(std::time::Duration::from_secs(5),async {
                        driver.initial(&mut frames).await;
                        let underlay=driver.frames.last().unwrap().clone();
                        let stack=handle.lock().unwrap().unwrap();
                        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(if menu {KeyCode::F(10)} else {KeyCode::F(3)}),true)],true).await;
                        let identity=stack.snapshot().top_id();
                        let rect=assert_anchor_bounds(published_popup_bounds(stack),stack,menu,width,height,false);
                        if at_left {assert_eq!(rect.left,0);left_edge+=1;}
                        else {
                            let cell=underlay.cell(usize::from(rect.left-1),usize::from(rect.top)).unwrap();
                            if pattern=="x" {assert_eq!(cell.text,"x");narrow+=1;}
                            else if cell.continuation {trailing+=1;} else {assert_eq!(cell.text,pattern,"fixture places a real wide span at the gutter");leading+=1;}
                        }
                        let label=format!("{width}x{height} menu={menu} pattern={pattern} offset={offset} left={at_left} open");
                        assert_gutter(driver.frames.last().unwrap(),raw.lock().unwrap().last().unwrap(),rect,&label);
                        assert_narrow_outside(driver.frames.last().unwrap(),rect,pattern);
                        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(TerminalEvent::Resize(new_width,new_height),true)],true).await;
                        assert_eq!(stack.snapshot().top_id(),identity,"resize keeps the same open overlay identity");
                        assert_eq!(stack.viewport(),(new_width,new_height));
                        let resized=assert_anchor_bounds(published_popup_bounds(stack),stack,menu,new_width,new_height,false);
                        assert_gutter(driver.frames.last().unwrap(),raw.lock().unwrap().last().unwrap(),resized,&format!("{label} resized"));
                        assert_narrow_outside(driver.frames.last().unwrap(),resized,pattern);
                        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Esc),true)],true).await;
                        assert_eq!(stack.snapshot().top_id(),SurfaceId::Screen);
                        if !at_left {
                            let cell=driver.frames.last().unwrap().cell(usize::from(resized.left-1),usize::from(resized.top)).unwrap();
                            assert!(cell.continuation || cell.text==pattern,"closing restores the actual patterned underlay");
                        }
                        let anchor=published_anchor(stack,menu);
                        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(click(anchor.left,anchor.top),true)],true).await;
                        assert_ne!(stack.snapshot().top_id(),identity);
                        let reopened=assert_anchor_bounds(published_popup_bounds(stack),stack,menu,new_width,new_height,false);
                        assert_eq!(reopened,resized);
                        assert_gutter(driver.frames.last().unwrap(),raw.lock().unwrap().last().unwrap(),reopened,&format!("{label} reopened"));
                        assert_narrow_outside(driver.frames.last().unwrap(),reopened,pattern);
                    }).await.unwrap();
                }
            }
        }
    }
    eprintln!(
        "QUALIFIED_SC25 leading={leading} trailing={trailing} narrow={narrow} left_edge={left_edge}"
    );
    assert!(
        leading > 0 && trailing > 0 && narrow > 0 && left_edge > 0,
        "all wide-span parities, narrow outside text and x0 are actually qualified"
    );
}
#[tokio::test]
async fn fixed_overlay_gutter_repaints_a_wide_continuation_at_standard_grids() {
    for (width, height) in STANDARD_GRIDS {
        for glyph in ["📂", "✉️", "界"] {
            let (mut driver, config, journal) = EventDriver::new();
            let raw = Arc::new(Mutex::new(Vec::new()));
            let captured = raw.clone();
            let mut ui = element!(Scene(journal,width:Some(width),height:Some(height),wide_glyph:glyph.to_owned(),wide_glyph_left:Some(18)));
            let mut frames = Box::pin(
                recording_terminal_render_loop(&mut ui, config)
                    .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
            );
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                assert!(
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .cell(19, 4)
                        .unwrap()
                        .continuation,
                    "the fixed fixture starts a real two-cell glyph before the gutter"
                );
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::F(10)), true)],
                        true,
                    )
                    .await;
                let frame = driver.frames.last().unwrap();
                let canvas = raw.lock().unwrap();
                let canvas = canvas.last().unwrap();
                for row in 4..10 {
                    assert_eq!(
                        canvas.cell(19, row).unwrap().background_color,
                        Some(Theme::default().background(SurfaceRole::Raised)),
                        "fixed surface's raw canvas gutter is painted before serialization"
                    );
                    let cell = frame.cell(19, row).unwrap();
                    assert!(
                        !cell.continuation,
                        "serialized fixed gutter is no longer a wide-glyph continuation"
                    );
                    assert_eq!(
                        cell.style.background,
                        Some(Theme::default().background(SurfaceRole::Raised))
                    );
                }
            })
            .await
            .unwrap();
        }
    }
}

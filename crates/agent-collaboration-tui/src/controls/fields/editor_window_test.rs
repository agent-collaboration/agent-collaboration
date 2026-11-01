use super::editor_window_fixture::{
    WindowControl, WindowScene, assert_caret, assert_outside_unchanged, band, content, key,
};
use crate::test_support::{
    event_driver::{EventDriver, InputMode},
    grids::STANDARD_GRIDS,
    recording_backend::recording_terminal_render_loop,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;

const LONG_ASCII: &str = concat!(
    "123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    "123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    "123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-END",
);
const LONG_WIDE: &str = concat!(
    "東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語",
    "東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語",
    "東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語東京📂界語-END",
);

#[tokio::test]
async fn long_text_and_filter_queries_keep_a_whole_caret_and_adjacent_cells_at_all_grids() {
    for control in [WindowControl::Text, WindowControl::Filter] {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in STANDARD_GRIDS {
                for query in [LONG_ASCII, LONG_WIDE] {
                    let handle = Arc::new(Mutex::new(None));
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(WindowScene(journal,width,height,control:Some(control),handle:handle.clone()));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        driver.initial(&mut frames).await;
                        let fixture = handle.lock().unwrap().clone().unwrap();
                        let initial = driver.frames.last().unwrap().clone();
                        let area = band(&fixture, &initial);
                        assert_caret(&initial, area);
                        driver
                            .drive(
                                &mut frames,
                                mode,
                                vec![(TerminalEvent::BracketedPaste(query.into()), true)],
                                true,
                            )
                            .await;
                        assert_eq!(fixture.editor().text(), query);
                        assert_eq!(fixture.editor().cursor(), query.chars().count());
                        assert_eq!(
                            band(&fixture, driver.frames.last().unwrap()),
                            area,
                            "typing cannot steal adjacent toolbar cells"
                        );
                        let frame = driver.frames.last().unwrap();
                        assert_caret(frame, area);
                        assert!(
                            frame.row_text(area.row).contains("-END"),
                            "visible query tail at {width}x{height} {control:?} {mode:?}\n{}",
                            frame.row_text(area.row)
                        );
                        assert_outside_unchanged(&initial, frame, area);
                    })
                    .await
                    .unwrap();
                }
            }
        }
    }
}

#[tokio::test]
async fn editor_windows_stay_put_until_navigation_crosses_their_edges_for_fields_and_combo_queries()
{
    for control in [
        WindowControl::Text,
        WindowControl::Filter,
        WindowControl::Combo,
    ] {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in STANDARD_GRIDS {
                for query in [LONG_ASCII, LONG_WIDE] {
                    let handle = Arc::new(Mutex::new(None));
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(WindowScene(journal,width,height,control:Some(control),handle:handle.clone()));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    tokio::time::timeout(std::time::Duration::from_secs(5),async {
                        driver.initial(&mut frames).await;
                        let fixture=handle.lock().unwrap().clone().unwrap();
                        if control==WindowControl::Combo {
                            driver.drive(&mut frames,mode,vec![(key(KeyCode::Enter),true)],true).await;
                        }
                        driver.drive(&mut frames,mode,vec![(TerminalEvent::BracketedPaste(query.into()),true)],true).await;
                        let area=band(&fixture,driver.frames.last().unwrap());
                        let end=driver.frames.last().unwrap().clone();
                        let end_column=assert_caret(&end,area);
                        let visible=content(&end,area);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Left),true)],true).await;
                        let frame=driver.frames.last().unwrap();
                        assert_eq!(fixture.editor().cursor(),query.chars().count()-1);
                        assert_eq!(assert_caret(frame,area),end_column-1,"Left moves the caret within the existing window");
                        assert_eq!(content(frame,area),visible,"Left inside the window must not scroll its text at {width}x{height} {control:?} {mode:?}");
                        assert_outside_unchanged(&end,frame,area);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Right),true)],true).await;
                        assert_eq!(assert_caret(driver.frames.last().unwrap(),area),end_column);
                        assert_eq!(content(driver.frames.last().unwrap(),area),visible);
                        if control==WindowControl::Combo {
                            driver.drive(&mut frames,mode,vec![(key(KeyCode::Backspace),true)],true).await;
                            assert_eq!(fixture.editor().text(),query.chars().take(query.chars().count()-1).collect::<String>());
                            assert_eq!(fixture.editor().cursor(),query.chars().count()-1);
                            let area=band(&fixture,driver.frames.last().unwrap());
                            assert_caret(driver.frames.last().unwrap(),area);
                            let steps=(area.right-area.left+2).min(fixture.editor().cursor());
                            driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Left),true);steps],true).await;
                            assert_eq!(assert_caret(driver.frames.last().unwrap(),area),area.left,"crossing the left edge reveals the query cursor at {width}x{height} {mode:?},query={query:?},cursor={},steps={steps},area={area:?}\n{}",fixture.editor().cursor(),driver.frames.last().unwrap().row_text(area.row));
                            driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Right),true);steps],true).await;
                            assert_eq!(fixture.editor().cursor(),query.chars().count()-1);
                            assert_caret(driver.frames.last().unwrap(),area);
                            assert_eq!(fixture.combo.read().value().unwrap().as_str(),"alpha","query edits never commit a choice");
                            let remove=fixture.editor().cursor()-2;
                            driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Backspace),true);remove],true).await;
                            let short=query.chars().take(2).collect::<String>();
                            assert_eq!(fixture.editor().text(),short);
                            let area=band(&fixture,driver.frames.last().unwrap());
                            assert_eq!(assert_caret(driver.frames.last().unwrap(),area),area.left+short.width(),"a shortened query returns to its whole visible text");
                            return;
                        }
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Home),true)],true).await;
                        assert_eq!(fixture.editor().cursor(),0);
                        assert_eq!(assert_caret(driver.frames.last().unwrap(),area),area.left);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Right),true)],true).await;
                        assert_eq!(fixture.editor().cursor(),1);
                        assert_caret(driver.frames.last().unwrap(),area);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Backspace),true)],true).await;
                        assert_eq!(fixture.editor().text(),query.chars().skip(1).collect::<String>());
                        assert_eq!(fixture.editor().cursor(),0);
                        assert_caret(driver.frames.last().unwrap(),area);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::End),true)],true).await;
                        assert_caret(driver.frames.last().unwrap(),area);
                        driver.drive(&mut frames,mode,vec![(key(KeyCode::Backspace),true)],true).await;
                        assert_eq!(fixture.editor().text(),query.chars().skip(1).take(query.chars().count()-2).collect::<String>());
                        assert_eq!(fixture.editor().cursor(),query.chars().count()-2);
                        assert_caret(driver.frames.last().unwrap(),area);
                        let remove=fixture.editor().cursor()-2;
                        driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Backspace),true);remove],true).await;
                        let short=query.chars().skip(1).take(2).collect::<String>();
                        assert_eq!(fixture.editor().text(),short);
                        assert_eq!(assert_caret(driver.frames.last().unwrap(),area),area.left+short.width(),"a shortened field returns to its whole visible text");
                    }).await.unwrap();
                }
            }
        }
    }
}

#[tokio::test]
async fn eleven_cell_filter_scrolls_an_eighteen_character_query_before_the_toolbar_count() {
    let handle = Arc::new(Mutex::new(None));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(WindowScene(journal,width:32u16,height:24u16,control:Some(WindowControl::Filter),handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let fixture = handle.lock().unwrap().clone().unwrap();
        let before = driver.frames.last().unwrap().clone();
        let area = band(&fixture, &before);
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(
                    TerminalEvent::BracketedPaste("123456789ABCDEFGHI".into()),
                    true,
                )],
                true,
            )
            .await;
        let frame = driver.frames.last().unwrap();
        assert_eq!(fixture.editor().text(), "123456789ABCDEFGHI");
        assert_caret(frame, area);
        assert_eq!(
            area.right - area.left + 1,
            9,
            "the slash and gap leave nine editor cells within eleven outer cells"
        );
        assert!(
            frame.row_text(area.row).contains("FGHI"),
            "the real narrow viewport shows the query tail\n{}",
            frame.row_text(area.row)
        );
        assert_outside_unchanged(&before, frame, area);
    })
    .await
    .unwrap();
}

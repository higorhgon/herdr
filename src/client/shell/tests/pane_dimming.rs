use super::*;
use crate::terminal_theme::{DefaultColorKind, RgbColor};
use ratatui::style::Color;

fn split_surface() -> PaneSurfaceFrame {
    let mut surface = surface();
    let pane = surface.panes[0].clone();
    let half = |x| SurfaceRect {
        x,
        y: 0,
        width: 2,
        height: 2,
    };
    surface.panes = vec![
        PaneSurfacePane {
            rect: half(0),
            inner_rect: half(0),
            ..pane.clone()
        },
        PaneSurfacePane {
            pane_id: "pane_2".into(),
            rect: half(2),
            inner_rect: half(2),
            focused: false,
            ..pane
        },
    ];
    surface
}

fn dimmed_state(opacity: f32, surface: PaneSurfaceFrame) -> ClientShellState {
    let mut config = Config::default();
    config.ui.unfocused_pane_opacity = opacity;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface);
    state.handle_raw_events(vec![
        RawInputEvent::HostDefaultColor {
            kind: DefaultColorKind::Foreground,
            color: RgbColor {
                r: 200,
                g: 200,
                b: 200,
            },
        },
        RawInputEvent::HostDefaultColor {
            kind: DefaultColorKind::Background,
            color: RgbColor { r: 0, g: 0, b: 0 },
        },
    ]);
    state
}

fn pane_cell_fg(state: &mut ClientShellState, pane_index: usize) -> u32 {
    let frame = state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[pane_index].clone();
    let index =
        usize::from(pane.inner_rect.y) * usize::from(frame.width) + usize::from(pane.inner_rect.x);
    frame.cells[index].fg
}

#[test]
fn unfocused_panes_fade_toward_the_host_background() {
    let mut state = dimmed_state(0.7, split_surface());

    assert_eq!(
        pane_cell_fg(&mut state, 0),
        crate::protocol::color_to_u32(Color::Reset)
    );
    assert_eq!(
        pane_cell_fg(&mut state, 1),
        crate::protocol::color_to_u32(Color::Rgb(140, 140, 140))
    );
}

#[test]
fn dimming_stays_off_by_default_and_for_a_lone_pane() {
    let mut state = dimmed_state(1.0, split_surface());
    assert_eq!(
        pane_cell_fg(&mut state, 1),
        crate::protocol::color_to_u32(Color::Reset)
    );

    let mut state = dimmed_state(0.7, surface());
    assert_eq!(
        pane_cell_fg(&mut state, 0),
        crate::protocol::color_to_u32(Color::Reset)
    );
}

#[test]
fn retained_patches_keep_unfocused_panes_faded() {
    let mut state = dimmed_state(0.7, split_surface());
    let composed = state.compose(106, 20).expect("composed frame");
    let layout = state.layout(106, 20);
    let current = state.pane_surface.clone().expect("pane surface");
    let cell = crate::protocol::CellData {
        symbol: "N".into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    };
    // One row in each pane: x 0..2 is the focused pane, x 2..4 the other.
    let patch = crate::protocol::PaneSurfacePatch {
        boot_id: current.boot_id.clone(),
        projection_revision: current.projection_revision,
        base_surface_revision: current.surface_revision,
        surface_revision: current.surface_revision + 1,
        rows: vec![
            crate::protocol::PaneSurfacePatchRow {
                x: 0,
                y: 0,
                cells: vec![cell.clone(); 2],
            },
            crate::protocol::PaneSurfacePatchRow {
                x: 2,
                y: 0,
                cells: vec![cell; 2],
            },
        ],
        panes: current.panes.clone(),
        cursor: None,
    };

    let ClientPaneSurfacePatchOutcome::Applied(Some(patch)) = state.apply_pane_surface_patch(patch)
    else {
        panic!("expected fast retained patch");
    };
    let patched = apply_composed_surface_patch(&composed, patch).expect("apply composed patch");
    let at = |x: u16| {
        &patched.cells[usize::from(layout.pane_surface.y) * usize::from(patched.width)
            + usize::from(layout.pane_surface.x + x)]
    };

    assert_eq!(at(0).symbol, "N");
    assert_eq!(at(0).fg, crate::protocol::color_to_u32(Color::Reset));
    assert_eq!(at(2).symbol, "N");
    assert_eq!(
        at(2).fg,
        crate::protocol::color_to_u32(Color::Rgb(140, 140, 140))
    );
}

fn backdrop_cell(opacity: f32) -> crate::protocol::CellData {
    let mut state = dimmed_state(opacity, split_surface());
    state.overlay = Some(ClientShellOverlay::Help(ClientHelpOverlay {
        query: TextEditor::default(),
        search_focused: true,
        scroll: 0,
    }));
    let frame = state.compose(240, 80).expect("composed frame");
    let pane = state.hits.panes[0].clone();
    frame.cells
        [usize::from(pane.inner_rect.y) * usize::from(frame.width) + usize::from(pane.inner_rect.x)]
    .clone()
}

#[test]
fn modals_fade_their_backdrop_like_unfocused_panes() {
    // Even the focused pane fades behind a modal, by blending, not faint text.
    let cell = backdrop_cell(0.7);
    assert_eq!(
        cell.fg,
        crate::protocol::color_to_u32(Color::Rgb(140, 140, 140))
    );
    assert_eq!(cell.modifier & Modifier::DIM.bits(), 0);

    // With dimming off, modals keep Herdr's faint backdrop.
    let cell = backdrop_cell(1.0);
    assert_eq!(cell.fg, crate::protocol::color_to_u32(Color::Reset));
    assert_ne!(cell.modifier & Modifier::DIM.bits(), 0);
}

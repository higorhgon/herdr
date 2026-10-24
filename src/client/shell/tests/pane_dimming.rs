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

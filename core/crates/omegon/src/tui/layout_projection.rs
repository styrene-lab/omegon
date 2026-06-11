//! Ratatui layout planning for high-level TUI surfaces.
//!
//! This module turns shared surface state plus frame-local measurements into
//! concrete terminal rectangles. It is intentionally TUI/Ratatui-specific; the
//! shared `surfaces::layout::UiSurfaces` remains renderer-neutral.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::surfaces::layout::UiSurfaces;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayoutInputs {
    pub area: Rect,
    pub surfaces: UiSurfaces,
    pub focus_mode: bool,
    pub dashboard_has_content: bool,
    pub editor_height: u16,
    pub editor_info_height: u16,
    pub instrument_footer_height: u16,
    pub status_height: u16,
    pub pending_permission: bool,
    pub active_tool_stream_height: u16,
    pub plan_dock_height: u16,
    pub segment_detail_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayoutPlan {
    pub show_dashboard: bool,
    pub is_slim: bool,
    pub main_area: Rect,
    pub dashboard_area: Option<Rect>,
    pub conversation_area: Rect,
    pub active_tool_stream_area: Rect,
    pub permission_lane_area: Rect,
    pub plan_dock_area: Rect,
    pub segment_detail_area: Rect,
    pub editor_area: Rect,
    pub editor_info_area: Rect,
    pub status_area: Rect,
    pub footer_area: Rect,
    pub footer_height: u16,
    pub active_tool_stream_height: u16,
    pub permission_lane_height: u16,
    pub plan_dock_height: u16,
    pub segment_detail_height: u16,
}

fn project_dashboard_height(inputs: TuiLayoutInputs, show_dashboard: bool) -> u16 {
    if show_dashboard && !inputs.focus_mode {
        1
    } else {
        0
    }
}

pub fn plan_tui_layout(inputs: TuiLayoutInputs) -> TuiLayoutPlan {
    let show_dashboard = inputs.surfaces.dashboard && inputs.dashboard_has_content;
    let main_area = inputs.area;
    let dashboard_area = None;

    let footer_height = if inputs.focus_mode || !inputs.surfaces.footer {
        0
    } else if inputs.surfaces.instruments {
        inputs.instrument_footer_height
    } else {
        1
    };

    let is_slim = inputs.surfaces.is_compact() && !inputs.focus_mode;
    let status_height = if is_slim { inputs.status_height } else { 0 };
    let permission_lane_height = if is_slim && inputs.pending_permission {
        2
    } else {
        0
    };
    let mut active_tool_stream_height = if is_slim {
        inputs.active_tool_stream_height
    } else {
        0
    };
    let mut plan_dock_height = if is_slim { inputs.plan_dock_height } else { 0 };
    let mut segment_detail_height = inputs.segment_detail_height;

    if permission_lane_height > 0 {
        active_tool_stream_height = active_tool_stream_height.min(6);
        plan_dock_height = plan_dock_height.min(4);
    }

    let fixed_without_conversation = inputs
        .editor_height
        .saturating_add(inputs.editor_info_height)
        .saturating_add(status_height)
        .saturating_add(footer_height)
        .saturating_add(permission_lane_height)
        .saturating_add(segment_detail_height);
    let bottom_budget = main_area
        .height
        .saturating_sub(fixed_without_conversation)
        .saturating_sub(3);
    if segment_detail_height > bottom_budget {
        segment_detail_height = bottom_budget;
    }
    if active_tool_stream_height.saturating_add(plan_dock_height) > bottom_budget {
        plan_dock_height = plan_dock_height.min(bottom_budget);
        let stream_budget = bottom_budget.saturating_sub(plan_dock_height);
        active_tool_stream_height = active_tool_stream_height.min(stream_budget);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(active_tool_stream_height),
            Constraint::Length(permission_lane_height),
            Constraint::Length(segment_detail_height),
            Constraint::Length(inputs.editor_height),
            Constraint::Length(inputs.editor_info_height),
            Constraint::Length(plan_dock_height),
            Constraint::Length(status_height),
            Constraint::Length(project_dashboard_height(inputs, show_dashboard)),
            Constraint::Length(footer_height),
        ])
        .split(main_area);

    TuiLayoutPlan {
        show_dashboard,
        is_slim,
        main_area,
        dashboard_area,
        conversation_area: chunks[0],
        active_tool_stream_area: chunks[1],
        permission_lane_area: chunks[2],
        segment_detail_area: chunks[3],
        editor_area: chunks[4],
        editor_info_area: chunks[5],
        plan_dock_area: chunks[6],
        status_area: chunks[7],
        footer_area: chunks[9],
        footer_height,
        active_tool_stream_height,
        permission_lane_height,
        plan_dock_height,
        segment_detail_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lean_layout_hides_dashboard_and_footer() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 40),
            surfaces: UiSurfaces::lean(),
            focus_mode: false,
            dashboard_has_content: true,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 0,
            plan_dock_height: 0,
            segment_detail_height: 0,
        });
        assert!(!plan.show_dashboard);
        assert!(plan.is_slim);
        assert_eq!(plan.footer_height, 0);
        assert_eq!(plan.status_area.height, 2);
    }

    #[test]
    fn full_layout_uses_thin_bottom_project_strip_when_populated() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 140, 40),
            surfaces: UiSurfaces::full(),
            focus_mode: false,
            dashboard_has_content: true,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 0,
            plan_dock_height: 0,
            segment_detail_height: 0,
        });
        assert!(plan.show_dashboard);
        assert!(!plan.is_slim);
        assert_eq!(plan.dashboard_area, None);
        assert_eq!(plan.main_area, Rect::new(0, 0, 140, 40));
        assert_eq!(plan.footer_height, 4);
        assert_eq!(plan.status_area.height, 0);
        assert_eq!(plan.footer_area.y, 36);
        assert_eq!(plan.footer_area.height, 4);
        assert_eq!(plan.conversation_area.y, 0);
    }

    #[test]
    fn slim_layout_preserves_bottom_status_stack() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 36),
            surfaces: UiSurfaces::lean(),
            focus_mode: false,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 3,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 6,
            plan_dock_height: 4,
            segment_detail_height: 3,
        });
        assert_eq!(plan.footer_height, 0);
        assert_eq!(plan.status_area.height, 2);
        assert_eq!(plan.plan_dock_area.height, 4);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_area.y < plan.editor_info_area.y);
        assert!(plan.editor_info_area.y < plan.plan_dock_area.y);
        assert!(plan.plan_dock_area.y < plan.status_area.y);
    }

    #[test]
    fn permission_lane_caps_slim_auxiliary_heights() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 30),
            surfaces: UiSurfaces {
                dashboard: false,
                instruments: false,
                footer: true,
            },
            focus_mode: false,
            dashboard_has_content: false,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: true,
            active_tool_stream_height: 20,
            plan_dock_height: 20,
            segment_detail_height: 0,
        });
        assert!(plan.is_slim);
        assert_eq!(plan.permission_lane_height, 2);
        assert!(plan.active_tool_stream_height <= 6);
        assert!(plan.plan_dock_height <= 4);
    }

    #[test]
    fn segment_detail_area_reserves_bottom_space() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 32),
            surfaces: UiSurfaces::lean(),
            focus_mode: false,
            dashboard_has_content: false,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 0,
            plan_dock_height: 0,
            segment_detail_height: 8,
        });
        assert_eq!(plan.segment_detail_height, 8);
        assert_eq!(plan.segment_detail_area.height, 8);
        assert!(plan.conversation_area.height >= 3);
    }

    #[test]
    fn slim_plan_is_pinned_between_editor_and_status_bar() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 40),
            surfaces: UiSurfaces::lean(),
            focus_mode: false,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 5,
            plan_dock_height: 5,
            segment_detail_height: 4,
        });

        assert_eq!(plan.plan_dock_height, 5);
        assert!(plan.active_tool_stream_area.y < plan.segment_detail_area.y);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_area.y < plan.editor_info_area.y);
        assert!(plan.editor_info_area.y < plan.plan_dock_area.y);
        assert!(plan.plan_dock_area.y < plan.status_area.y);
    }

    #[test]
    fn slim_layout_preserves_editor_and_status_under_auxiliary_pressure() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 18),
            surfaces: UiSurfaces::lean(),
            focus_mode: false,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 4,
            status_height: 2,
            pending_permission: false,
            active_tool_stream_height: 12,
            plan_dock_height: 5,
            segment_detail_height: 4,
        });

        assert!(plan.conversation_area.height >= 3);
        assert_eq!(plan.editor_area.height, 4);
        assert_eq!(plan.editor_info_area.height, 1);
        assert_eq!(plan.status_area.height, 2);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_info_area.y < plan.plan_dock_area.y);
        assert!(plan.plan_dock_area.y < plan.status_area.y);
    }
}

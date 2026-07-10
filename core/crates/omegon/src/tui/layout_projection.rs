//! Ratatui layout planning for high-level TUI surfaces.
//!
//! This module turns shared surface state plus frame-local measurements into
//! concrete terminal rectangles. It is intentionally TUI/Ratatui-specific; the
//! shared `surfaces::layout::UiSurfaces` remains renderer-neutral.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::surfaces::layout::{UiPresentationLevel, UiSurfaces};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayoutInputs {
    pub area: Rect,
    pub surfaces: UiSurfaces,
    pub presentation_level: UiPresentationLevel,
    pub dashboard_has_content: bool,
    pub editor_height: u16,
    pub editor_info_height: u16,
    pub instrument_footer_height: u16,
    pub session_height: u16,
    pub pending_permission: bool,
    pub tool_inspection_height: u16,
    pub workbench_height: u16,
    pub segment_detail_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayoutPlan {
    pub show_dashboard: bool,
    pub is_slim: bool,
    pub main_area: Rect,
    pub dashboard_area: Option<Rect>,
    pub conversation_area: Rect,
    pub tool_inspection_area: Rect,
    pub permission_lane_area: Rect,
    pub workbench_area: Rect,
    pub segment_detail_area: Rect,
    pub editor_area: Rect,
    pub editor_info_area: Rect,
    pub session_area: Rect,
    pub footer_area: Rect,
    pub footer_height: u16,
    pub tool_inspection_height: u16,
    pub permission_lane_height: u16,
    pub workbench_height: u16,
    pub segment_detail_height: u16,
}

fn project_dashboard_height(_inputs: TuiLayoutInputs, show_dashboard: bool) -> u16 {
    if show_dashboard { 1 } else { 0 }
}

pub fn plan_tui_layout(inputs: TuiLayoutInputs) -> TuiLayoutPlan {
    let show_dashboard = inputs.surfaces.dashboard && inputs.dashboard_has_content;
    let main_area = inputs.area;
    let dashboard_area = None;

    let footer_height = if !inputs.surfaces.footer {
        0
    } else if inputs.surfaces.instruments {
        inputs.instrument_footer_height
    } else {
        1
    };

    let is_slim = inputs.presentation_level != UiPresentationLevel::Full;
    let session_height = if is_slim { inputs.session_height } else { 0 };
    let permission_lane_height = if is_slim && inputs.pending_permission {
        2
    } else {
        0
    };
    let mut tool_inspection_height = if is_slim {
        inputs.tool_inspection_height
    } else {
        0
    };
    let mut workbench_height = inputs.workbench_height;
    let mut segment_detail_height = inputs.segment_detail_height;

    if permission_lane_height > 0 {
        tool_inspection_height = tool_inspection_height.min(8);
        workbench_height = workbench_height.min(8);
    }

    let fixed_without_conversation = inputs
        .editor_height
        .saturating_add(inputs.editor_info_height)
        .saturating_add(session_height)
        .saturating_add(footer_height)
        .saturating_add(permission_lane_height);
    let available_auxiliary = main_area
        .height
        .saturating_sub(fixed_without_conversation)
        .saturating_sub(3);
    if segment_detail_height > available_auxiliary {
        segment_detail_height = available_auxiliary;
    }
    let bottom_budget = available_auxiliary.saturating_sub(segment_detail_height);
    if tool_inspection_height.saturating_add(workbench_height) > bottom_budget {
        // Share constrained space proportionally instead of allowing the plan to
        // consume the entire budget and starving live operations (or vice versa).
        let desired_total = tool_inspection_height.saturating_add(workbench_height);
        let tool_share = if desired_total == 0 {
            0
        } else {
            ((bottom_budget as u32 * tool_inspection_height as u32) / desired_total as u32) as u16
        };
        tool_inspection_height = tool_inspection_height.min(tool_share);
        workbench_height =
            workbench_height.min(bottom_budget.saturating_sub(tool_inspection_height));
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(tool_inspection_height),
            Constraint::Length(permission_lane_height),
            Constraint::Length(segment_detail_height),
            Constraint::Length(inputs.editor_height),
            Constraint::Length(inputs.editor_info_height),
            Constraint::Length(workbench_height),
            Constraint::Length(session_height),
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
        tool_inspection_area: chunks[1],
        permission_lane_area: chunks[2],
        segment_detail_area: chunks[3],
        editor_area: chunks[4],
        editor_info_area: chunks[5],
        workbench_area: chunks[6],
        session_area: chunks[7],
        footer_area: chunks[9],
        footer_height,
        tool_inspection_height,
        permission_lane_height,
        workbench_height,
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
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: true,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 0,
            workbench_height: 0,
            segment_detail_height: 0,
        });
        assert!(!plan.show_dashboard);
        assert!(plan.is_slim);
        assert_eq!(plan.footer_height, 0);
        assert_eq!(plan.session_area.height, 2);
    }

    #[test]
    fn full_layout_uses_thin_bottom_project_strip_when_populated() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 140, 40),
            surfaces: UiSurfaces::full(),
            presentation_level: UiPresentationLevel::Full,
            dashboard_has_content: true,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 0,
            workbench_height: 0,
            segment_detail_height: 0,
        });
        assert!(plan.show_dashboard);
        assert!(!plan.is_slim);
        assert_eq!(plan.dashboard_area, None);
        assert_eq!(plan.main_area, Rect::new(0, 0, 140, 40));
        assert_eq!(plan.footer_height, 4);
        assert_eq!(plan.session_area.height, 0);
        assert_eq!(plan.footer_area.y, 36);
        assert_eq!(plan.footer_area.height, 4);
        assert_eq!(plan.conversation_area.y, 0);
    }

    #[test]
    fn slim_layout_preserves_bottom_status_stack() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 36),
            surfaces: UiSurfaces::lean(),
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 3,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 6,
            workbench_height: 4,
            segment_detail_height: 3,
        });
        assert_eq!(plan.footer_height, 0);
        assert_eq!(plan.session_area.height, 2);
        assert_eq!(plan.workbench_area.height, 4);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_area.y < plan.editor_info_area.y);
        assert!(plan.editor_info_area.y < plan.workbench_area.y);
        assert!(plan.workbench_area.y < plan.session_area.y);
    }

    #[test]
    fn permission_lane_caps_slim_auxiliary_heights() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 30),
            surfaces: UiSurfaces {
                dashboard: false,
                instruments: false,
                footer: true,
                activity: true,
            },
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: true,
            tool_inspection_height: 20,
            workbench_height: 20,
            segment_detail_height: 0,
        });
        assert!(plan.is_slim);
        assert_eq!(plan.permission_lane_height, 2);
        assert!(plan.tool_inspection_height <= 8);
        assert!(plan.workbench_height <= 8);
    }

    #[test]
    fn constrained_layout_shares_space_between_activity_and_plan() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 30),
            surfaces: UiSurfaces::lean(),
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 16,
            workbench_height: 16,
            segment_detail_height: 0,
        });
        assert!(plan.tool_inspection_height >= 9, "{plan:?}");
        assert!(plan.workbench_height >= 9, "{plan:?}");
        assert!(plan.conversation_area.height >= 3);
    }

    #[test]
    fn segment_detail_area_reserves_bottom_space() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 100, 32),
            surfaces: UiSurfaces::lean(),
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 3,
            editor_info_height: 0,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 0,
            workbench_height: 0,
            segment_detail_height: 8,
        });
        assert_eq!(plan.segment_detail_height, 8);
        assert_eq!(plan.segment_detail_area.height, 8);
        assert!(plan.conversation_area.height >= 3);
    }

    #[test]
    fn workbench_is_pinned_between_editor_and_status_bar() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 40),
            surfaces: UiSurfaces::lean(),
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 5,
            workbench_height: 5,
            segment_detail_height: 4,
        });

        assert_eq!(plan.workbench_height, 5);
        assert!(plan.tool_inspection_area.y < plan.segment_detail_area.y);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_area.y < plan.editor_info_area.y);
        assert!(plan.editor_info_area.y < plan.workbench_area.y);
        assert!(plan.workbench_area.y < plan.session_area.y);
    }

    #[test]
    fn slim_layout_preserves_editor_and_status_under_auxiliary_pressure() {
        let plan = plan_tui_layout(TuiLayoutInputs {
            area: Rect::new(0, 0, 120, 18),
            surfaces: UiSurfaces::lean(),
            presentation_level: UiPresentationLevel::Om,
            dashboard_has_content: false,
            editor_height: 4,
            editor_info_height: 1,
            instrument_footer_height: 4,
            session_height: 2,
            pending_permission: false,
            tool_inspection_height: 12,
            workbench_height: 5,
            segment_detail_height: 4,
        });

        assert!(plan.conversation_area.height >= 3);
        assert_eq!(plan.editor_area.height, 4);
        assert_eq!(plan.editor_info_area.height, 1);
        assert_eq!(plan.session_area.height, 2);
        assert!(plan.segment_detail_area.y < plan.editor_area.y);
        assert!(plan.editor_info_area.y < plan.workbench_area.y);
        assert!(plan.workbench_area.y < plan.session_area.y);
    }
}

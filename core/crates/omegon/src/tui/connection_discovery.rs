//! Explicit connection discovery. Public model lookup never submits a draft.

use super::App;
use crate::providers::zen::{self, FreeModel};
use crate::surfaces::menu::{
    MenuActionProjection, MenuGroupProjection, MenuProjection, MenuRowKind, MenuRowProjection,
    MenuTabProjection,
};
use tokio::sync::oneshot;

pub(super) struct FreeModelDiscovery {
    task: tokio::task::JoinHandle<()>,
    result: oneshot::Receiver<Result<Vec<FreeModel>, String>>,
}

impl Drop for FreeModelDiscovery {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn model_menu(
    id: &str,
    title: &str,
    summary: &str,
    rows: Vec<MenuRowProjection>,
) -> MenuProjection {
    let mut menu = MenuProjection::new(id, title);
    menu.summary = Some(summary.into());
    menu.footer = Some("↑/↓ navigate · Enter select · / filter · Esc close".into());
    menu.tabs = vec![MenuTabProjection {
        id: "models".into(),
        label: "Models".into(),
        groups: vec![MenuGroupProjection {
            id: "models".into(),
            label: "Available models".into(),
            description: None,
            rows,
        }],
    }];
    menu
}

fn model_row(route: String, name: String, description: String) -> MenuRowProjection {
    MenuRowProjection {
        id: format!("connect.model.{route}"),
        label: name,
        description,
        value: Some(route.clone()),
        kind: MenuRowKind::Object,
        badges: vec![],
        metadata: vec![],
        primary_action: Some(MenuActionProjection::command(
            format!("connect.select.{route}"),
            "Use this model",
            format!("/model {route}"),
        )),
        actions: vec![],
        safety: None,
        availability: None,
    }
}

fn free_models_menu(result: Result<Vec<FreeModel>, String>) -> MenuProjection {
    let (summary, rows) = match result {
        Ok(models) if !models.is_empty() => (
            "Free · OpenCode Zen. Selecting a model connects to this hosted service under the data terms shown below. No paid fallback.".into(),
            models.into_iter().map(|model| model_row(
                format!("{}:{}", zen::PROVIDER_ID, model.id),
                model.name.into(),
                model.privacy_notice.into(),
            )).collect(),
        ),
        Ok(_) => ("No supported free models are available right now. Reopen Free hosted models to retry, or choose another connection.".into(), vec![]),
        Err(error) => (format!("Free model discovery failed: {error}. Reopen Free hosted models to retry, or choose another connection."), vec![]),
    };
    model_menu("auth.free", "Free hosted models", &summary, rows)
}

impl App {
    pub(super) fn open_free_model_menu(&mut self) {
        self.free_model_discovery = None;
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.open_menu_projection(free_models_menu(Err("runtime unavailable".into())));
            return;
        };
        let (sender, result) = oneshot::channel();
        let task = runtime.spawn(async move {
            let models = zen::refresh_models()
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(models);
        });
        self.free_model_discovery = Some(FreeModelDiscovery { task, result });
        self.open_menu_projection(model_menu(
            "auth.free.loading",
            "Free hosted models",
            "Checking OpenCode Zen availability…",
            vec![],
        ));
    }

    /// Poll without blocking input; closing/replacing the menu cancels its lookup.
    pub(super) fn poll_free_model_discovery(&mut self) -> bool {
        if self.free_model_discovery.is_none() {
            return false;
        }
        if self
            .active_menu
            .as_ref()
            .is_none_or(|menu| menu.projection.id != "auth.free.loading")
        {
            self.free_model_discovery = None;
            return false;
        }
        let result = match self
            .free_model_discovery
            .as_mut()
            .unwrap()
            .result
            .try_recv()
        {
            Ok(result) => result,
            Err(oneshot::error::TryRecvError::Empty) => return false,
            Err(oneshot::error::TryRecvError::Closed) => Err("lookup stopped".into()),
        };
        self.free_model_discovery = None;
        self.open_menu_projection(free_models_menu(result));
        true
    }

    pub(super) fn open_local_model_menu(&mut self) {
        let catalog = crate::model_catalog::ModelCatalog::discover();
        let rows = catalog
            .providers
            .into_values()
            .flatten()
            .filter(|model| model.available && model.execution_class.as_deref() == Some("local"))
            .map(|model| model_row(model.id, model.name, model.description))
            .collect::<Vec<_>>();
        let summary = if rows.is_empty() {
            "No local models discovered. Start a local inference server with a coding model, then reopen Local models."
        } else {
            "Models served locally. Select a model to use this connection."
        };
        self.open_menu_projection(model_menu("auth.local", "Local models", summary, rows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_model_choice_discloses_terms_and_only_dispatches_explicit_route() {
        let model = zen::model("big-pickle").unwrap().clone();
        let menu = free_models_menu(Ok(vec![model]));
        let row = &menu.tabs[0].groups[0].rows[0];
        assert!(row.description.contains("used to improve the model"));
        assert!(row.description.contains("temporary"));
        assert_eq!(
            row.primary_action.as_ref().unwrap().command.as_deref(),
            Some("/model opencode-zen:big-pickle")
        );
        assert!(menu.summary.unwrap().contains("No paid fallback"));
    }

    #[test]
    fn selecting_free_model_preserves_draft_until_authoritative_route_update() {
        let mut app = super::super::tests::test_app();
        app.settings.lock().unwrap().provider_connected = false;
        app.editor.set_text("private draft stays here");
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let menu = free_models_menu(Ok(vec![zen::model("big-pickle").unwrap().clone()]));
        let action = menu.tabs[0].groups[0].rows[0]
            .primary_action
            .clone()
            .unwrap();
        app.open_menu_projection(menu);
        assert!(rx.try_recv().is_err());
        app.execute_active_menu_action(action, &tx);
        assert!(
            matches!(rx.try_recv(), Ok(super::super::TuiCommand::SetModel { model, .. }) if model == "opencode-zen:big-pickle")
        );
        assert!(
            rx.try_recv().is_err(),
            "selecting must not enqueue the private draft"
        );
        assert_eq!(app.editor.render_text(), "private draft stays here");
        assert!(
            !app.settings().provider_connected,
            "coordinator owns readiness"
        );
    }

    #[test]
    fn failed_or_empty_discovery_never_offers_static_or_paid_models() {
        for result in [Ok(vec![]), Err("offline".into())] {
            let menu = free_models_menu(result);
            assert!(menu.tabs[0].groups[0].rows.is_empty());
            assert!(menu.summary.unwrap().contains("another connection"));
        }
    }

    #[tokio::test]
    async fn closing_free_discovery_cancels_lookup_without_reopening_menu() {
        let mut app = super::super::tests::test_app();
        let (_sender, result) = oneshot::channel();
        let task = tokio::spawn(std::future::pending());
        let aborted = task.abort_handle();
        app.free_model_discovery = Some(FreeModelDiscovery { task, result });
        app.open_auth_menu();
        app.poll_free_model_discovery();
        assert!(app.free_model_discovery.is_none());
        tokio::task::yield_now().await;
        assert!(aborted.is_finished());
        assert_eq!(app.active_menu.as_ref().unwrap().projection.id, "auth");
    }
}

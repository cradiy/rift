use std::{sync::Arc, time::Duration};

use gpui::{AppContext, Context};
use rift_core::{
    application::{NavigationEffect, NavigationMessage, NavigationState},
    ports::NavigationSource,
};

const DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct NavigationController {
    state: NavigationState,
    source: Arc<dyn NavigationSource>,
}

impl NavigationController {
    pub(crate) fn new(source: Arc<dyn NavigationSource>, cx: &mut Context<Self>) -> Self {
        let mut controller = Self {
            state: NavigationState::default(),
            source,
        };
        controller.dispatch(NavigationMessage::Refresh, cx);
        controller.start_device_refresh(cx);
        controller
    }

    pub(crate) fn state(&self) -> &NavigationState {
        &self.state
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.dispatch(NavigationMessage::Refresh, cx);
    }

    fn dispatch(&mut self, message: NavigationMessage, cx: &mut Context<Self>) {
        let effects = self.state.update(message);
        cx.notify();
        for effect in effects {
            self.execute(effect, cx);
        }
    }

    fn execute(&self, effect: NavigationEffect, cx: &mut Context<Self>) {
        let NavigationEffect::Load { request_id } = effect;
        let source = self.source.clone();
        let load = cx.background_spawn(async move { source.load() });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |controller, cx| {
                let message = match result {
                    Ok(snapshot) => NavigationMessage::Loaded {
                        request_id,
                        snapshot,
                    },
                    Err(error) => NavigationMessage::LoadFailed { request_id, error },
                };
                controller.dispatch(message, cx);
            });
        })
        .detach();
    }

    fn start_device_refresh(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(DEVICE_REFRESH_INTERVAL)
                    .await;
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.update(cx, |controller, cx| {
                    controller.dispatch(NavigationMessage::Refresh, cx);
                });
            }
        })
        .detach();
    }
}

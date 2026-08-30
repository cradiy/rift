use crate::{domain::NavigationSnapshot, ports::NavigationError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NavigationLoadState {
    Idle,
    Loading { request_id: u64 },
    Failed { message: String },
}

#[derive(Clone, Debug)]
pub struct NavigationState {
    snapshot: NavigationSnapshot,
    load_state: NavigationLoadState,
    next_request_id: u64,
}

#[derive(Clone, Debug)]
pub enum NavigationMessage {
    Refresh,
    Loaded {
        request_id: u64,
        snapshot: NavigationSnapshot,
    },
    LoadFailed {
        request_id: u64,
        error: NavigationError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationEffect {
    Load { request_id: u64 },
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            snapshot: NavigationSnapshot::default(),
            load_state: NavigationLoadState::Idle,
            next_request_id: 0,
        }
    }
}

impl NavigationState {
    pub fn snapshot(&self) -> &NavigationSnapshot {
        &self.snapshot
    }

    pub fn load_state(&self) -> &NavigationLoadState {
        &self.load_state
    }

    pub fn update(&mut self, message: NavigationMessage) -> Vec<NavigationEffect> {
        match message {
            NavigationMessage::Refresh => {
                let request_id = self.next_request_id;
                self.next_request_id = self.next_request_id.wrapping_add(1);
                self.load_state = NavigationLoadState::Loading { request_id };
                vec![NavigationEffect::Load { request_id }]
            }
            NavigationMessage::Loaded {
                request_id,
                snapshot,
            } if self.is_current_request(request_id) => {
                self.snapshot = snapshot;
                self.load_state = NavigationLoadState::Idle;
                Vec::new()
            }
            NavigationMessage::LoadFailed { request_id, error }
                if self.is_current_request(request_id) =>
            {
                self.load_state = NavigationLoadState::Failed {
                    message: error.to_string(),
                };
                Vec::new()
            }
            NavigationMessage::Loaded { .. } | NavigationMessage::LoadFailed { .. } => Vec::new(),
        }
    }

    fn is_current_request(&self, request_id: u64) -> bool {
        matches!(
            self.load_state,
            NavigationLoadState::Loading {
                request_id: pending_id
            } if pending_id == request_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_stale_navigation_results() {
        let mut state = NavigationState::default();
        let first = state.update(NavigationMessage::Refresh);
        let second = state.update(NavigationMessage::Refresh);
        let NavigationEffect::Load {
            request_id: first_id,
        } = first[0];
        let NavigationEffect::Load {
            request_id: second_id,
        } = second[0];

        state.update(NavigationMessage::Loaded {
            request_id: first_id,
            snapshot: NavigationSnapshot::default(),
        });
        assert_eq!(
            state.load_state(),
            &NavigationLoadState::Loading {
                request_id: second_id
            }
        );
    }
}

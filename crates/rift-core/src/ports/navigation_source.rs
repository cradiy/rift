use std::{error::Error, fmt};

use crate::domain::NavigationSnapshot;

pub trait NavigationSource: Send + Sync + 'static {
    fn load(&self) -> Result<NavigationSnapshot, NavigationError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationError {
    pub message: String,
}

impl NavigationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NavigationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for NavigationError {}

use super::Value;
use crate::RuleModelError;
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct HostPool {
    addresses: Arc<[Value]>,
    cursor: Arc<AtomicUsize>,
    selection: OnceLock<usize>,
}

impl HostPool {
    pub fn new(addresses: Vec<Value>) -> Result<Self, RuleModelError> {
        if addresses.is_empty() {
            return Err(RuleModelError::empty(
                "host addresses",
                "host requires at least one address",
            ));
        }
        if addresses.iter().any(|address| match address {
            Value::Inline(value) | Value::File(value) | Value::Reference(value) => {
                value.trim().is_empty()
            }
        }) {
            return Err(RuleModelError::empty(
                "host address",
                "host addresses cannot be empty",
            ));
        }
        Ok(Self {
            addresses: addresses.into(),
            cursor: Arc::new(AtomicUsize::new(0)),
            selection: OnceLock::new(),
        })
    }

    pub fn addresses(&self) -> &[Value] {
        &self.addresses
    }

    pub fn selected_address(&self) -> &Value {
        let index = *self
            .selection
            .get_or_init(|| self.cursor.fetch_add(1, Ordering::Relaxed));
        &self.addresses[index % self.addresses.len()]
    }
}

impl Clone for HostPool {
    fn clone(&self) -> Self {
        Self {
            addresses: self.addresses.clone(),
            cursor: self.cursor.clone(),
            selection: OnceLock::new(),
        }
    }
}

impl fmt::Debug for HostPool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostPool")
            .field("addresses", &self.addresses)
            .finish_non_exhaustive()
    }
}

impl PartialEq for HostPool {
    fn eq(&self, other: &Self) -> bool {
        self.addresses == other.addresses
    }
}

impl Eq for HostPool {}

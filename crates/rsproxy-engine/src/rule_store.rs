use arc_swap::ArcSwap;
use rsproxy_rules::{RuleError, RuleSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod storage;
mod watch;

use storage::{
    atomic_write, group_path, io_error, load_snapshot, persist_manifest, validate_group_name,
};
pub(crate) use watch::RuleWatchHandle;
pub use watch::RuleWatchStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
/// One ordered, independently enabled rule source persisted under the rules directory.
pub struct RuleGroup {
    /// Stable group identifier used for storage and control API lookup.
    pub name: String,
    /// Whether this group's rules participate in the compiled active set.
    pub enabled: bool,
    /// Original DSL text preserved for editing and export.
    pub text: String,
}

#[derive(Clone, Debug)]
/// Immutable ordered rules view published atomically to request handlers.
pub struct RuleSnapshot {
    /// All configured groups, including disabled groups, in evaluation order.
    pub groups: Vec<RuleGroup>,
    /// Compiled rule set containing only enabled groups.
    pub compiled: RuleSet,
}

#[derive(Clone)]
/// Cloneable rule repository with atomic reader snapshots and serialized writes.
///
/// An update is compiled and persisted before publication, so readers retain a
/// valid previous snapshot if validation or storage fails.
pub struct RuleStore {
    inner: Arc<RuleStoreInner>,
}

struct RuleStoreInner {
    rules_dir: PathBuf,
    snapshot: ArcSwap<RuleSnapshot>,
    update: Mutex<()>,
    watch_status: Mutex<RuleWatchStatus>,
}

#[derive(Debug)]
/// Validation, compilation, persistence or watcher failure from [`RuleStore`].
pub enum RuleStoreError {
    /// A group name or requested mutation violates a store invariant.
    Invalid(String),
    /// The requested named group does not exist.
    NotFound(String),
    /// One or more DSL diagnostics prevented snapshot compilation.
    Parse(Vec<RuleError>),
    /// A filesystem operation failed before a new snapshot could be published.
    Io {
        /// Description of the failed storage operation.
        context: String,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// Filesystem watching could not be created or maintained.
    Watch(notify::Error),
}

impl RuleStore {
    /// Validates a group name using the same rules as persistent mutations.
    pub fn validate_name(name: &str) -> Result<(), RuleStoreError> {
        validate_group_name(name)
    }

    /// Loads all persisted groups and compiles the initial atomic snapshot.
    ///
    /// Missing storage is treated as an empty/default store; malformed existing
    /// rules are returned rather than silently discarded.
    pub fn load(storage: &Path) -> Result<Self, RuleStoreError> {
        let rules_dir = storage.join("rules");
        let snapshot = load_snapshot(&rules_dir)?;
        Ok(Self::from_snapshot(rules_dir, snapshot))
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn from_compiled(storage: &Path, compiled: RuleSet) -> Self {
        Self::from_snapshot(
            storage.join("rules"),
            RuleSnapshot {
                groups: vec![RuleGroup {
                    name: "default".to_string(),
                    enabled: true,
                    text: String::new(),
                }],
                compiled,
            },
        )
    }

    fn from_snapshot(rules_dir: PathBuf, snapshot: RuleSnapshot) -> Self {
        Self {
            inner: Arc::new(RuleStoreInner {
                rules_dir,
                snapshot: ArcSwap::from_pointee(snapshot),
                update: Mutex::new(()),
                watch_status: Mutex::new(RuleWatchStatus::default()),
            }),
        }
    }

    /// Clones the currently published immutable snapshot without blocking writers.
    pub fn snapshot(&self) -> Arc<RuleSnapshot> {
        self.inner.snapshot.load_full()
    }

    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// Creates or replaces a group after compiling the complete prospective snapshot.
    pub fn set_group(&self, name: &str, text: String) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        let _update = self.inner.update.lock().expect("rule store lock poisoned");
        let current = self.snapshot();
        let mut groups = current.groups.clone();
        match groups.iter_mut().find(|group| group.name == name) {
            Some(group) => group.text = text,
            None => groups.push(RuleGroup {
                name: name.to_string(),
                enabled: true,
                text,
            }),
        }
        let snapshot = RuleSnapshot::compile(groups)?;
        fs::create_dir_all(&self.inner.rules_dir)
            .map_err(|source| io_error("create rules directory", source))?;
        let group = snapshot
            .groups
            .iter()
            .find(|group| group.name == name)
            .expect("updated rule group must exist after insertion");
        atomic_write(
            &group_path(&self.inner.rules_dir, name),
            group.text.as_bytes(),
        )?;
        persist_manifest(&self.inner.rules_dir, &snapshot.groups)?;
        let snapshot = Arc::new(snapshot);
        self.inner.snapshot.store(snapshot.clone());
        Ok(snapshot)
    }

    /// Removes a non-default group and publishes the remaining compiled rules.
    pub fn remove_group(&self, name: &str) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        if name == "default" {
            return Err(RuleStoreError::Invalid(
                "the default rule group cannot be removed".to_string(),
            ));
        }
        let _update = self.inner.update.lock().expect("rule store lock poisoned");
        let current = self.snapshot();
        if !current.groups.iter().any(|group| group.name == name) {
            return Err(RuleStoreError::NotFound(name.to_string()));
        }
        let groups = current
            .groups
            .iter()
            .filter(|group| group.name != name)
            .cloned()
            .collect();
        let snapshot = RuleSnapshot::compile(groups)?;
        persist_manifest(&self.inner.rules_dir, &snapshot.groups)?;
        match fs::remove_file(group_path(&self.inner.rules_dir, name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error("remove rule group", source)),
        }
        let snapshot = Arc::new(snapshot);
        self.inner.snapshot.store(snapshot.clone());
        Ok(snapshot)
    }

    /// Enables or disables a group without changing its stored DSL text or order.
    pub fn set_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        let _update = self.inner.update.lock().expect("rule store lock poisoned");
        let current = self.snapshot();
        let mut groups = current.groups.clone();
        let group = groups
            .iter_mut()
            .find(|group| group.name == name)
            .ok_or_else(|| RuleStoreError::NotFound(name.to_string()))?;
        group.enabled = enabled;
        let snapshot = RuleSnapshot::compile(groups)?;
        persist_manifest(&self.inner.rules_dir, &snapshot.groups)?;
        let snapshot = Arc::new(snapshot);
        self.inner.snapshot.store(snapshot.clone());
        Ok(snapshot)
    }
}

impl RuleSnapshot {
    fn compile(groups: Vec<RuleGroup>) -> Result<Self, RuleStoreError> {
        RuleSet::parse_groups(
            groups
                .iter()
                .map(|group| (group.name.as_str(), group.text.as_str())),
        )
        .map_err(RuleStoreError::Parse)?;
        let compiled = RuleSet::parse_groups(
            groups
                .iter()
                .filter(|group| group.enabled)
                .map(|group| (group.name.as_str(), group.text.as_str())),
        )
        .map_err(RuleStoreError::Parse)?;
        Ok(Self { groups, compiled })
    }

    /// Finds a group by its stable name, including disabled groups.
    pub fn group(&self, name: &str) -> Option<&RuleGroup> {
        self.groups.iter().find(|group| group.name == name)
    }
}

impl fmt::Display for RuleStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::NotFound(group) => write!(f, "rule group `{group}` not found"),
            Self::Parse(errors) => {
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        f.write_str("\n")?;
                    }
                    write!(f, "{error}")?;
                }
                Ok(())
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Watch(source) => write!(f, "rule watcher: {source}"),
        }
    }
}

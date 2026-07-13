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
pub struct RuleGroup {
    pub name: String,
    pub enabled: bool,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct RuleSnapshot {
    pub groups: Vec<RuleGroup>,
    pub compiled: RuleSet,
}

#[derive(Clone)]
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
pub enum RuleStoreError {
    Invalid(String),
    NotFound(String),
    Parse(Vec<RuleError>),
    Io { context: String, source: io::Error },
    Watch(notify::Error),
}

impl RuleStore {
    pub fn validate_name(name: &str) -> Result<(), RuleStoreError> {
        validate_group_name(name)
    }

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

    pub fn snapshot(&self) -> Arc<RuleSnapshot> {
        self.inner.snapshot.load_full()
    }

    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    pub fn set_group(&self, name: &str, text: String) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        let _update = self.inner.update.lock().expect("rule store poisoned");
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
            .expect("updated group should exist");
        atomic_write(
            &group_path(&self.inner.rules_dir, name),
            group.text.as_bytes(),
        )?;
        persist_manifest(&self.inner.rules_dir, &snapshot.groups)?;
        let snapshot = Arc::new(snapshot);
        self.inner.snapshot.store(snapshot.clone());
        Ok(snapshot)
    }

    pub fn remove_group(&self, name: &str) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        if name == "default" {
            return Err(RuleStoreError::Invalid(
                "the default rule group cannot be removed".to_string(),
            ));
        }
        let _update = self.inner.update.lock().expect("rule store poisoned");
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

    pub fn set_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<Arc<RuleSnapshot>, RuleStoreError> {
        validate_group_name(name)?;
        let _update = self.inner.update.lock().expect("rule store poisoned");
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

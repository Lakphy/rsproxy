use super::*;
use crate::upstream_pool::{ActivityStore, KeyedActivity, PoolWaitSpec, acquire_slot};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Condvar, OnceLock};

const THREAD_IDLE_CAPACITY: usize = 8;
const FAST_POOL_IDLE_TTL: Duration = Duration::from_secs(90);

pub(super) struct FastConnection {
    pub(super) reader: BufReader<TcpStream>,
    pub(super) writer: TcpStream,
    pool_key: String,
    _permit: FastPermit,
    last_used: Instant,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl FastConnection {
    pub(super) fn new(stream: TcpStream, permit: FastPermit) -> io::Result<Self> {
        let pool_key = permit.key.clone();
        Ok(Self {
            reader: BufReader::with_capacity(16 * 1024, stream.try_clone()?),
            writer: stream,
            pool_key,
            _permit: permit,
            last_used: Instant::now(),
            read_timeout: None,
            write_timeout: None,
        })
    }

    pub(super) fn set_read_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        if self.read_timeout == Some(timeout) {
            return Ok(());
        }
        self.reader.get_ref().set_read_timeout(Some(timeout))?;
        self.read_timeout = Some(timeout);
        Ok(())
    }

    pub(super) fn set_write_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        if self.write_timeout == Some(timeout) {
            return Ok(());
        }
        self.writer.set_write_timeout(Some(timeout))?;
        self.write_timeout = Some(timeout);
        Ok(())
    }
}

#[derive(Default)]
struct ThreadPool {
    entries: VecDeque<FastConnection>,
}

thread_local! {
    static THREAD_POOL: RefCell<ThreadPool> = RefCell::new(ThreadPool::default());
}

struct ActiveState {
    inner: Mutex<KeyedActivity>,
    available: Condvar,
}

pub(super) struct FastPermit {
    key: String,
}

pub(super) fn acquire(key: &str, limit: usize, timeout: Duration) -> io::Result<FastPermit> {
    let state = active_state();
    let started = Instant::now();
    acquire_slot(
        &state.inner,
        &state.available,
        key,
        limit,
        timeout,
        started,
        PoolWaitSpec {
            stage: "upstream_h1",
            limit_label: "active limit",
        },
    )?;
    Ok(FastPermit {
        key: key.to_string(),
    })
}

pub(super) fn checkout(key: &str) -> Option<FastConnection> {
    THREAD_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        let index = pool
            .entries
            .iter()
            .position(|connection| connection.pool_key == key)?;
        let connection = pool.entries.remove(index)?;
        if connection.last_used.elapsed() >= FAST_POOL_IDLE_TTL {
            None
        } else {
            Some(connection)
        }
    })
}

pub(super) fn checkin(key: &str, mut connection: FastConnection) {
    debug_assert_eq!(connection.pool_key, key);
    connection.last_used = Instant::now();
    THREAD_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        pool.entries
            .retain(|pooled| pooled.pool_key.as_str() != key);
        while pool.entries.len() >= THREAD_IDLE_CAPACITY {
            pool.entries.pop_front();
        }
        pool.entries.push_back(connection);
    });
}

impl Drop for FastPermit {
    fn drop(&mut self) {
        let state = active_state();
        let mut active = state.inner.lock().unwrap();
        active.release(&self.key);
        drop(active);
        state.available.notify_one();
    }
}

fn active_state() -> &'static ActiveState {
    static STATE: OnceLock<ActiveState> = OnceLock::new();
    STATE.get_or_init(|| ActiveState {
        inner: Mutex::new(KeyedActivity::default()),
        available: Condvar::new(),
    })
}

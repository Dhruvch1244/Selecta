//! The one scheduler for background work (pillar 2). Every analysis, tagging
//! or lookup job goes through here; nothing else spawns its own thread.
//!
//! Shape: a fixed worker pool pulling from one priority queue.
//! - **Dedup**: a job is keyed; submitting a key that is queued or running
//!   is a no-op, so "scan twice" never analyses a file twice.
//! - **Priority**: `Now` (the user is waiting, e.g. a track was just
//!   dropped on a deck) jumps ahead of `Idle` (background library analysis),
//!   decided when a worker frees up, not at submit time.
//! - **Cancellation**: cooperative. Each job gets a token and checks it
//!   between expensive stages; cancelling a group drops its queued jobs too.
//!
//! Progress is a snapshot the UI polls; workers never call into the UI.

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use serde::Serialize;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[ts(export)]
pub enum Priority {
    Idle = 0,
    Next = 1,
    Now = 2,
}

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

type Work = Box<dyn FnOnce(&CancelToken) + Send>;

struct Queued {
    priority: Priority,
    seq: u64,
    key: String,
    group: String,
    token: CancelToken,
    work: Work,
}

impl PartialEq for Queued {
    fn eq(&self, o: &Self) -> bool {
        self.priority == o.priority && self.seq == o.seq
    }
}
impl Eq for Queued {}
impl PartialOrd for Queued {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Queued {
    // Higher priority first; FIFO within a priority (lower seq first).
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&o.priority).then_with(|| o.seq.cmp(&self.seq))
    }
}

#[derive(Default)]
struct State {
    queue: BinaryHeap<Queued>,
    live: HashSet<String>,
    groups: HashMap<String, Vec<CancelToken>>,
    group_totals: HashMap<String, (u64, u64)>, // (done, total)
    shutdown: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupProgress {
    pub group: String,
    pub done: u64,
    pub total: u64,
}

pub struct Scheduler {
    state: Arc<(Mutex<State>, Condvar)>,
    seq: AtomicU64,
}

impl Scheduler {
    pub fn new(workers: usize) -> Arc<Self> {
        let s = Arc::new(Scheduler { state: Arc::new((Mutex::new(State::default()), Condvar::new())), seq: AtomicU64::new(0) });
        for i in 0..workers.max(1) {
            let st = s.state.clone();
            thread::Builder::new().name(format!("selecta-job-{i}")).spawn(move || worker(st)).expect("spawn worker");
        }
        s
    }

    /// Returns false if an identical key is already queued or running.
    pub fn submit(&self, key: &str, group: &str, priority: Priority, work: impl FnOnce(&CancelToken) + Send + 'static) -> bool {
        let (lock, cv) = &*self.state;
        let mut st = lock.lock().unwrap();
        if !st.live.insert(key.to_string()) {
            return false;
        }
        let token = CancelToken::default();
        st.groups.entry(group.to_string()).or_default().push(token.clone());
        st.group_totals.entry(group.to_string()).or_default().1 += 1;
        st.queue.push(Queued {
            priority,
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            key: key.to_string(),
            group: group.to_string(),
            token,
            work: Box::new(work),
        });
        cv.notify_one();
        true
    }

    pub fn cancel_group(&self, group: &str) {
        let (lock, _) = &*self.state;
        let mut st = lock.lock().unwrap();
        for t in st.groups.remove(group).unwrap_or_default() {
            t.cancel();
        }
        st.group_totals.remove(group);
    }

    pub fn progress(&self) -> Vec<GroupProgress> {
        let st = self.state.0.lock().unwrap();
        let mut v: Vec<_> = st.group_totals.iter().map(|(g, (d, t))| GroupProgress { group: g.clone(), done: *d, total: *t }).collect();
        v.sort_by(|a, b| a.group.cmp(&b.group));
        v
    }

    pub fn idle(&self) -> bool {
        self.state.0.lock().unwrap().live.is_empty()
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        let (lock, cv) = &*self.state;
        lock.lock().unwrap().shutdown = true;
        cv.notify_all();
    }
}

fn worker(state: Arc<(Mutex<State>, Condvar)>) {
    let (lock, cv) = &*state;
    loop {
        let job = {
            let mut st = lock.lock().unwrap();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(j) = st.queue.pop() {
                    break j;
                }
                st = cv.wait(st).unwrap();
            }
        };
        if !job.token.is_cancelled() {
            // A panicking job must not take the worker (or the pool) down with it.
            let token = job.token.clone();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (job.work)(&token)));
        }
        let mut st = lock.lock().unwrap();
        st.live.remove(&job.key);
        if let Some(t) = st.group_totals.get_mut(&job.group) {
            t.0 += 1;
            if t.0 >= t.1 {
                st.group_totals.remove(&job.group);
                st.groups.remove(&job.group);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn dedups_and_prioritises() {
        let s = Scheduler::new(1);
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        // Block the single worker so the next submissions queue up.
        let g = gate.clone();
        s.submit("block", "g", Priority::Idle, move |_| {
            let (l, c) = &*g;
            let mut open = l.lock().unwrap();
            while !*open {
                open = c.wait(open).unwrap();
            }
        });
        thread::sleep(Duration::from_millis(20));
        for (k, p) in [("a", Priority::Idle), ("b", Priority::Now), ("c", Priority::Next)] {
            let tx = tx.clone();
            assert!(s.submit(k, "g", p, move |_| tx.send(k).unwrap()));
        }
        assert!(!s.submit("a", "g", Priority::Now, |_| {}), "duplicate key must be rejected");
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let order: Vec<_> = (0..3).map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap()).collect();
        assert_eq!(order, vec!["b", "c", "a"]);
    }

    #[test]
    fn cancelled_jobs_do_not_run() {
        let s = Scheduler::new(1);
        let ran = Arc::new(AtomicBool::new(false));
        s.submit("slow", "x", Priority::Now, |_| thread::sleep(Duration::from_millis(50)));
        let r = ran.clone();
        s.submit("victim", "y", Priority::Idle, move |_| r.store(true, Ordering::SeqCst));
        s.cancel_group("y");
        thread::sleep(Duration::from_millis(150));
        assert!(!ran.load(Ordering::SeqCst));
    }
}

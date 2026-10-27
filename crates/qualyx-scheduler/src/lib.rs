//! Supervisor primitives: a task DAG executed with bounded concurrency, a wall-clock budget,
//! cancellation and "dependents of a failure are skipped", plus single-writer git worktrees.
//!
//! Agents are roles, not processes (master prompt): a task is any `FnOnce` closure, so a
//! deterministic worker and an LLM-backed one are scheduled the same way.

pub mod worktree;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use qualyx_runners::CancelToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    Succeeded,
    Failed(String),
    /// Not started: a dependency failed, the budget ran out, or the run was cancelled.
    Skipped(String),
}

impl TaskOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, TaskOutcome::Succeeded)
    }
}

type Work = Box<dyn FnOnce(&CancelToken) -> Result<(), String> + Send>;

struct Task {
    deps: Vec<String>,
    work: Work,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_concurrency: usize,
    /// Tasks not started before this elapses are skipped; running ones get the cancel signal.
    pub budget: Option<Duration>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_concurrency: 1,
            budget: None,
        }
    }
}

#[derive(Default)]
pub struct Dag {
    tasks: BTreeMap<String, Task>,
}

impl Dag {
    pub fn new() -> Dag {
        Dag::default()
    }

    pub fn add(
        &mut self,
        id: impl Into<String>,
        deps: &[&str],
        work: impl FnOnce(&CancelToken) -> Result<(), String> + Send + 'static,
    ) -> &mut Self {
        self.tasks.insert(
            id.into(),
            Task {
                deps: deps.iter().map(|d| d.to_string()).collect(),
                work: Box::new(work),
            },
        );
        self
    }

    /// Reject unknown dependencies and cycles before anything runs.
    pub fn validate(&self) -> Result<(), String> {
        for (id, t) in &self.tasks {
            for d in &t.deps {
                if !self.tasks.contains_key(d) {
                    return Err(format!("task `{id}` depends on unknown task `{d}`"));
                }
            }
        }
        // Kahn's algorithm: anything left over is on a cycle.
        let mut indeg: BTreeMap<&str, usize> = self
            .tasks
            .iter()
            .map(|(k, t)| (k.as_str(), t.deps.len()))
            .collect();
        let mut ready: Vec<&str> = indeg
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut seen = 0;
        while let Some(n) = ready.pop() {
            seen += 1;
            for (k, t) in &self.tasks {
                if t.deps.iter().any(|d| d == n) {
                    let e = indeg.get_mut(k.as_str()).expect("known task");
                    *e -= 1;
                    if *e == 0 {
                        ready.push(k);
                    }
                }
            }
        }
        if seen != self.tasks.len() {
            let stuck: Vec<&str> = indeg
                .iter()
                .filter(|(_, n)| **n > 0)
                .map(|(k, _)| *k)
                .collect();
            return Err(format!("dependency cycle among: {}", stuck.join(", ")));
        }
        Ok(())
    }

    /// Execute; returns every task's outcome. Deterministic order of start among ready tasks
    /// (lexicographic), concurrency never above `limits.max_concurrency`.
    pub fn run(
        mut self,
        limits: &Limits,
        cancel: &CancelToken,
    ) -> Result<BTreeMap<String, TaskOutcome>, String> {
        self.validate()?;
        let start = Instant::now();
        let max = limits.max_concurrency.max(1);
        let mut outcomes: BTreeMap<String, TaskOutcome> = BTreeMap::new();
        let mut running: BTreeSet<String> = BTreeSet::new();
        let (tx, rx) = mpsc::channel::<(String, TaskOutcome)>();
        // Token handed to tasks: fires on external cancel or budget exhaustion.
        let task_cancel = CancelToken::default();

        loop {
            let over_budget = limits.budget.is_some_and(|b| start.elapsed() >= b);
            if cancel.is_cancelled() || over_budget {
                task_cancel.cancel();
            }
            // Skip tasks whose dependencies failed or were skipped, or that can no longer start.
            let ids: Vec<String> = self.tasks.keys().cloned().collect();
            for id in &ids {
                let t = &self.tasks[id];
                let blocked = t
                    .deps
                    .iter()
                    .find(|d| outcomes.get(*d).is_some_and(|o| !o.is_success()));
                let reason = if let Some(d) = blocked {
                    Some(format!("dependency `{d}` did not succeed"))
                } else if cancel.is_cancelled() {
                    Some("cancelled".to_string())
                } else if over_budget {
                    Some("budget exhausted".to_string())
                } else {
                    None
                };
                if let Some(r) = reason {
                    self.tasks.remove(id);
                    outcomes.insert(id.clone(), TaskOutcome::Skipped(r));
                }
            }
            // Start ready tasks up to the limit.
            let ready: Vec<String> = self
                .tasks
                .iter()
                .filter(|(_, t)| {
                    t.deps
                        .iter()
                        .all(|d| outcomes.get(d).is_some_and(TaskOutcome::is_success))
                })
                .map(|(k, _)| k.clone())
                .collect();
            for id in ready {
                if running.len() >= max {
                    break;
                }
                let task = self.tasks.remove(&id).expect("ready task exists");
                let (tx, token, tid) = (tx.clone(), task_cancel.clone(), id.clone());
                thread::spawn(move || {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (task.work)(&token)
                    }));
                    let outcome = match r {
                        Ok(Ok(())) => TaskOutcome::Succeeded,
                        Ok(Err(e)) => TaskOutcome::Failed(e),
                        Err(_) => TaskOutcome::Failed("task panicked".into()),
                    };
                    let _ = tx.send((tid, outcome));
                });
                running.insert(id);
            }
            if running.is_empty() && self.tasks.is_empty() {
                break;
            }
            if running.is_empty() {
                // Nothing running and nothing startable: only possible after validate() if
                // every remaining task waits on a skipped one, which the loop above handles.
                continue;
            }
            if let Ok((id, outcome)) = rx.recv_timeout(Duration::from_millis(100)) {
                running.remove(&id);
                outcomes.insert(id, outcome);
            }
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn respects_dependencies_and_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut dag = Dag::new();
        for (id, deps) in [("c", vec!["b"]), ("b", vec!["a"]), ("a", vec![])] {
            let log = log.clone();
            dag.add(id, &deps, move |_| {
                log.lock().unwrap().push(id);
                Ok(())
            });
        }
        let out = dag
            .run(
                &Limits {
                    max_concurrency: 4,
                    budget: None,
                },
                &CancelToken::default(),
            )
            .unwrap();
        assert!(out.values().all(TaskOutcome::is_success));
        assert_eq!(*log.lock().unwrap(), ["a", "b", "c"]);
    }

    #[test]
    fn never_exceeds_concurrency() {
        let (now, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let mut dag = Dag::new();
        for i in 0..8 {
            let (now, peak) = (now.clone(), peak.clone());
            dag.add(format!("t{i}"), &[], move |_| {
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(60));
                now.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            });
        }
        let t = Instant::now();
        dag.run(
            &Limits {
                max_concurrency: 3,
                budget: None,
            },
            &CancelToken::default(),
        )
        .unwrap();
        assert_eq!(peak.load(Ordering::SeqCst), 3);
        // 8 tasks / 3 lanes of 60 ms each needs at least 3 rounds.
        assert!(t.elapsed() >= Duration::from_millis(170));
    }

    #[test]
    fn failure_skips_dependents_but_not_siblings() {
        let mut dag = Dag::new();
        dag.add("build", &[], |_| Err("compile error".into()))
            .add("test", &["build"], |_| Ok(()))
            .add("report", &["test"], |_| Ok(()))
            .add("lint", &[], |_| Ok(()))
            .add("boom", &[], |_| panic!("bug"));
        let out = dag
            .run(&Limits::default(), &CancelToken::default())
            .unwrap();
        assert_eq!(out["build"], TaskOutcome::Failed("compile error".into()));
        assert!(matches!(&out["test"], TaskOutcome::Skipped(r) if r.contains("build")));
        assert!(matches!(&out["report"], TaskOutcome::Skipped(r) if r.contains("test")));
        assert_eq!(out["lint"], TaskOutcome::Succeeded);
        assert_eq!(out["boom"], TaskOutcome::Failed("task panicked".into()));
    }

    #[test]
    fn rejects_cycles_and_unknown_deps() {
        let mut dag = Dag::new();
        dag.add("a", &["b"], |_| Ok(()))
            .add("b", &["a"], |_| Ok(()));
        assert!(
            dag.run(&Limits::default(), &CancelToken::default())
                .unwrap_err()
                .contains("cycle")
        );
        let mut dag = Dag::new();
        dag.add("a", &["ghost"], |_| Ok(()));
        assert!(dag.validate().unwrap_err().contains("ghost"));
    }

    #[test]
    fn budget_signals_running_and_skips_pending() {
        let mut dag = Dag::new();
        dag.add("long", &[], |c| {
            while !c.is_cancelled() {
                thread::sleep(Duration::from_millis(20));
            }
            Err("stopped by budget".into())
        })
        .add("after", &["long"], |_| Ok(()));
        let t = Instant::now();
        let out = dag
            .run(
                &Limits {
                    max_concurrency: 1,
                    budget: Some(Duration::from_millis(300)),
                },
                &CancelToken::default(),
            )
            .unwrap();
        assert!(t.elapsed() < Duration::from_secs(5));
        assert_eq!(out["long"], TaskOutcome::Failed("stopped by budget".into()));
        assert!(matches!(&out["after"], TaskOutcome::Skipped(_)));
    }

    #[test]
    fn external_cancel_skips_everything_not_started() {
        let cancel = CancelToken::default();
        cancel.cancel();
        let mut dag = Dag::new();
        dag.add("a", &[], |_| Ok(()));
        let out = dag.run(&Limits::default(), &cancel).unwrap();
        assert_eq!(out["a"], TaskOutcome::Skipped("cancelled".into()));
    }
}

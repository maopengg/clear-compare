//! Scoped synchronous-worker control. No global task registry or runtime threads.
use crate::{Error, Result};
use clear_contracts::{OperationKind, TaskHandle, TaskPhase, TaskProgress};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct State {
    phase: TaskPhase,
    pause: bool,
    stage: String,
    path: Option<String>,
    completed: usize,
    read: u64,
    written: u64,
    error: Option<String>,
    epoch: u64,
    elapsed_end: Option<u128>,
}
pub struct Control {
    handle: TaskHandle,
    total: usize,
    cancel: Arc<AtomicBool>,
    started: AtomicBool,
    start: Instant,
    state: Mutex<State>,
    wake: Condvar,
}
struct Binding {
    control: Arc<Control>,
    critical: usize,
}
thread_local! {static CURRENT:RefCell<Option<Binding>>=const {RefCell::new(None)};}
struct Scope {
    old: Option<Binding>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| *c.borrow_mut() = self.old.take());
    }
}
pub struct Mutation {
    control: Option<Arc<Control>>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Mutation {
    fn drop(&mut self) {
        if let Some(control) = &self.control {
            CURRENT.with(|c| {
                if let Some(binding) = c.borrow_mut().as_mut()
                    && Arc::ptr_eq(&binding.control, control)
                {
                    binding.critical = binding.critical.saturating_sub(1);
                }
            });
        }
    }
}
fn clipped(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = value[..end].to_owned();
    if end < value.len() {
        out.push('…');
    }
    out
}
impl Control {
    pub fn new(kind: OperationKind, total: usize, cancel: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            handle: TaskHandle {
                id: crate::unique_id(),
                kind,
            },
            total,
            cancel,
            started: AtomicBool::new(false),
            start: Instant::now(),
            state: Mutex::new(State {
                phase: TaskPhase::Running,
                pause: false,
                stage: "queued".into(),
                path: None,
                completed: 0,
                read: 0,
                written: 0,
                error: None,
                epoch: 0,
                elapsed_end: None,
            }),
            wake: Condvar::new(),
        })
    }
    pub fn handle(&self) -> TaskHandle {
        self.handle.clone()
    }
    pub fn status(&self) -> TaskProgress {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        TaskProgress {
            handle: self.handle(),
            phase: s.phase,
            stage: s.stage.clone(),
            current_path: s.path.clone(),
            completed_actions: s.completed,
            total_actions: self.total,
            bytes_read: s.read.to_string(),
            bytes_written: s.written.to_string(),
            elapsed_ms: s
                .elapsed_end
                .unwrap_or_else(|| self.start.elapsed().as_millis())
                .to_string(),
            error: s.error.clone(),
        }
    }
    pub fn pause(&self) -> Result<()> {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.phase.terminal() || s.phase == TaskPhase::CancelRequested {
            return Err(Error::Invalid("Task is no longer pausable".into()));
        }
        s.pause = true;
        if s.phase != TaskPhase::Paused {
            s.phase = TaskPhase::PauseRequested;
        }
        self.wake.notify_all();
        Ok(())
    }
    pub fn resume(&self) -> Result<()> {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.phase.terminal() || s.phase == TaskPhase::CancelRequested {
            return Err(Error::Invalid("Task is no longer resumable".into()));
        }
        s.pause = false;
        s.phase = TaskPhase::Running;
        self.wake.notify_all();
        Ok(())
    }
    pub fn cancel(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s.phase.terminal() {
            self.cancel.store(true, Ordering::Relaxed);
            s.pause = false;
            s.phase = TaskPhase::CancelRequested;
            self.wake.notify_all();
        }
    }
    pub fn finish(&self, phase: TaskPhase, error: Option<&str>) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s.phase.terminal() && phase.terminal() {
            s.elapsed_end = Some(self.start.elapsed().as_millis());
            s.phase = phase;
            s.pause = false;
            s.error = error.map(|s| clipped(s, 65536));
            s.stage = "finished".into();
            self.wake.notify_all();
        }
    }
    pub fn run<T>(self: &Arc<Self>, body: impl FnOnce() -> Result<T>) -> Result<T> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(Error::Invalid("Task control can only run once".into()));
        }
        let old = CURRENT.with(|c| {
            c.replace(Some(Binding {
                control: self.clone(),
                critical: 0,
            }))
        });
        let _scope = Scope {
            old,
            _thread: PhantomData,
        };
        checkpoint(&self.cancel)?;
        body()
    }
    fn checkpoint(&self) -> Result<()> {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                s.phase = TaskPhase::CancelRequested;
                return Err(Error::Cancelled);
            }
            if !s.pause {
                return Ok(());
            }
            if s.phase != TaskPhase::Paused {
                s.epoch = s.epoch.saturating_add(1);
            }
            s.phase = TaskPhase::Paused;
            self.wake.notify_all();
            let (next, _) = self
                .wake
                .wait_timeout(s, Duration::from_millis(100))
                .unwrap_or_else(|p| p.into_inner());
            s = next;
        }
    }
}
fn matching(cancel: &AtomicBool) -> Option<(Arc<Control>, bool)> {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|b| std::ptr::eq(Arc::as_ptr(&b.control.cancel), cancel))
            .map(|b| (b.control.clone(), b.critical > 0))
    })
}
pub(crate) fn checkpoint(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if let Some((control, false)) = matching(cancel) {
        control.checkpoint()?;
    }
    Ok(())
}
/// Pause before entering; checks within the returned guard can cancel but cannot pause.
pub(crate) fn mutation(cancel: &AtomicBool) -> Result<Mutation> {
    checkpoint(cancel)?;
    let control = matching(cancel).map(|(c, _)| c);
    if control.is_some() {
        CURRENT.with(|c| {
            if let Some(b) = c.borrow_mut().as_mut() {
                b.critical += 1;
            }
        });
    }
    Ok(Mutation {
        control,
        _thread: PhantomData,
    })
}
fn update(body: impl FnOnce(&mut State)) {
    CURRENT.with(|c| {
        if let Some(binding) = c.borrow().as_ref() {
            let mut state = binding
                .control
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            body(&mut state);
        }
    });
}
pub(crate) fn stage(stage: &str) {
    update(|s| s.stage = clipped(stage, 120));
}
pub(crate) fn action(path: &str) {
    update(|s| {
        s.path = Some(clipped(path, 8192));
        s.stage = "action".into();
    });
}
pub(crate) fn completed() {
    update(|s| s.completed = s.completed.saturating_add(1));
}
pub(crate) fn read(bytes: usize) {
    update(|s| s.read = s.read.saturating_add(bytes as u64));
}
pub(crate) fn written(bytes: usize) {
    update(|s| s.written = s.written.saturating_add(bytes as u64));
}

fn epoch() -> u64 {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|b| {
                b.control
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .epoch
            })
            .unwrap_or(0)
    })
}
/// Re-run pausable preflight if any acknowledged pause interrupted its evidence.
pub(crate) fn validated_mutation(
    cancel: &AtomicBool,
    mut verify: impl FnMut() -> Result<()>,
) -> Result<Mutation> {
    if matching(cancel).is_none() {
        verify()?;
        return Ok(Mutation {
            control: None,
            _thread: PhantomData,
        });
    }
    loop {
        let before = epoch();
        verify()?;
        let guard = mutation(cancel)?;
        if before == epoch() {
            return Ok(guard);
        }
        drop(guard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn paused(control: &Control) {
        let mut state = control.state.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.phase != TaskPhase::Paused {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "worker never acknowledged pause");
            state = control.wake.wait_timeout(state, remaining).unwrap().0;
        }
    }
    #[test]
    fn pause_is_acknowledged_at_a_checkpoint_and_resume_keeps_progress_bounded() {
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::File, 1, cancel.clone());
        let (ready, rx) = mpsc::sync_channel(0);
        let (go, next) = mpsc::sync_channel(0);
        let worker = control.clone();
        let join = std::thread::spawn(move || {
            worker.run(|| {
                action("one file");
                ready.send(()).unwrap();
                next.recv().unwrap();
                crate::compare::check_cancel(&cancel)?;
                read(20);
                written(10);
                completed();
                Ok(())
            })
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        control.pause().unwrap();
        go.send(()).unwrap();
        paused(&control);
        let p = control.status();
        assert_eq!(p.phase, TaskPhase::Paused);
        assert_eq!(p.completed_actions, 0);
        assert_eq!(p.bytes_written, "0");
        control.resume().unwrap();
        join.join().unwrap().unwrap();
        control.finish(TaskPhase::Completed, None);
        let p = control.status();
        assert_eq!(p.completed_actions, 1);
        assert_eq!(p.bytes_read, "20");
        assert_eq!(p.bytes_written, "10");
        assert!(control.pause().is_err());
        assert!(control.resume().is_err());
        control.cancel();
        assert_eq!(control.status().phase, TaskPhase::Completed);
        assert!(control.run(|| Ok(())).is_err());
    }
    #[test]
    fn cancelling_a_paused_worker_wakes_it_without_executing_following_writes() {
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::Recovery, 1, cancel.clone());
        control.pause().unwrap();
        let worker = control.clone();
        let wrote = Arc::new(AtomicBool::new(false));
        let changed = wrote.clone();
        let join = std::thread::spawn(move || {
            worker.run(|| {
                crate::compare::check_cancel(&cancel)?;
                changed.store(true, Ordering::Relaxed);
                Ok(())
            })
        });
        paused(&control);
        control.cancel();
        assert!(matches!(join.join().unwrap(), Err(Error::Cancelled)));
        assert!(!wrote.load(Ordering::Relaxed));
        control.finish(TaskPhase::Cancelled, None);
        assert_eq!(control.status().phase, TaskPhase::Cancelled);
    }
    #[test]
    fn mutation_sections_defer_pause_but_still_honor_cancellation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::File, 1, cancel.clone());
        let (ready, rx) = mpsc::sync_channel(0);
        let (go, next) = mpsc::sync_channel(0);
        let (checked, got) = mpsc::sync_channel(0);
        let worker = control.clone();
        let join = std::thread::spawn(move || {
            worker.run(|| {
                let guard = mutation(&cancel)?;
                ready.send(()).unwrap();
                next.recv().unwrap();
                crate::compare::check_cancel(&cancel)?;
                checked.send(()).unwrap();
                next.recv().unwrap();
                drop(guard);
                crate::compare::check_cancel(&cancel)
            })
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        control.pause().unwrap();
        go.send(()).unwrap();
        got.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(control.status().phase, TaskPhase::PauseRequested);
        go.send(()).unwrap();
        paused(&control);
        control.cancel();
        assert!(matches!(join.join().unwrap(), Err(Error::Cancelled)));
    }
    #[test]
    fn paused_preflight_evidence_is_rechecked_before_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        std::fs::write(&source, b"before").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::File, 1, cancel.clone());
        let worker = control.clone();
        let (ready, rx) = mpsc::sync_channel(0);
        let (go, next) = mpsc::sync_channel(0);
        let check = source.clone();
        let output = target.clone();
        let join = std::thread::spawn(move || {
            worker.run(|| {
                let mut first = true;
                let _guard = validated_mutation(&cancel, || {
                    if std::fs::read(&check)? != b"before" {
                        return Err(Error::Invalid("preflight expired".into()));
                    }
                    if first {
                        first = false;
                        ready.send(()).unwrap();
                        next.recv().unwrap();
                    }
                    Ok(())
                })?;
                std::fs::write(&output, b"not allowed")?;
                Ok(())
            })
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        control.pause().unwrap();
        go.send(()).unwrap();
        paused(&control);
        std::fs::write(source, b"external").unwrap();
        control.resume().unwrap();
        assert!(join.join().unwrap().is_err());
        assert!(!target.exists());
    }
    #[test]
    fn cancellation_is_not_deferred_by_a_mutation_guard() {
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::File, 1, cancel.clone());
        let result = control.run(|| {
            let _guard = mutation(&cancel)?;
            control.cancel();
            crate::compare::check_cancel(&cancel)
        });
        assert!(matches!(result, Err(Error::Cancelled)));
    }
    #[test]
    fn scoped_binding_is_restored_after_panic_and_unrelated_cancel_tokens_do_not_pause() {
        let control = Control::new(OperationKind::File, 1, Arc::new(AtomicBool::new(false)));
        let outcome = std::panic::catch_unwind(|| control.run::<()>(|| panic!("test panic")));
        assert!(outcome.is_err());
        read(100);
        assert_eq!(control.status().bytes_read, "0");
        let cancel = Arc::new(AtomicBool::new(false));
        let control = Control::new(OperationKind::File, 1, cancel);
        control
            .run(|| {
                control.pause()?;
                crate::compare::check_cancel(&AtomicBool::new(false))?;
                control.resume()?;
                Ok(())
            })
            .unwrap();
    }
}

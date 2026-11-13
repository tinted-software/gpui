use android_activity::AndroidAppWaker;
use gpui::{
    PlatformDispatcher, Priority, PriorityQueueReceiver, PriorityQueueSender, RunnableVariant,
    profiler,
};
use parking_lot::{Condvar, Mutex};
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const MIN_THREADS: usize = 2;

struct TimerEntry {
    due: Instant,
    seq: u64,
    runnable: RunnableVariant,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.seq == other.seq
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

struct TimerQueueState {
    heap: BinaryHeap<TimerEntry>,
    next_seq: u64,
    shutdown: bool,
}

struct TimerQueue {
    state: Mutex<TimerQueueState>,
    condvar: Condvar,
}

pub(crate) struct AndroidDispatcher {
    background_sender: PriorityQueueSender<RunnableVariant>,
    main_sender: PriorityQueueSender<RunnableVariant>,
    main_receiver: Mutex<PriorityQueueReceiver<RunnableVariant>>,
    timers: Arc<TimerQueue>,
    waker: Mutex<Option<AndroidAppWaker>>,
    main_thread_id: thread::ThreadId,
    _threads: Vec<thread::JoinHandle<()>>,
}

impl AndroidDispatcher {
    pub(crate) fn new(waker: Option<AndroidAppWaker>) -> Self {
        let (background_sender, background_receiver) = PriorityQueueReceiver::new();
        let (main_sender, main_receiver) = PriorityQueueReceiver::new();

        let thread_count = thread::available_parallelism()
            .map_or(MIN_THREADS, |count| count.get().max(MIN_THREADS));

        let mut threads = (0..thread_count)
            .map(|index| {
                let receiver: PriorityQueueReceiver<RunnableVariant> = background_receiver.clone();
                thread::Builder::new()
                    .name(format!("AndroidWorker-{index}"))
                    .spawn(move || {
                        for runnable in receiver.iter() {
                            let location = runnable.metadata().location;
                            let spawned = runnable.metadata().spawned;
                            profiler::update_running_task(spawned, location);
                            runnable.run();
                            profiler::save_task_timing();
                        }
                    })
                    .expect("failed to spawn Android background worker thread")
            })
            .collect::<Vec<_>>();

        let timers = Arc::new(TimerQueue {
            state: Mutex::new(TimerQueueState {
                heap: BinaryHeap::new(),
                next_seq: 0,
                shutdown: false,
            }),
            condvar: Condvar::new(),
        });

        let timer_thread = thread::Builder::new()
            .name("AndroidTimer".to_owned())
            .spawn({
                let timers = Arc::clone(&timers);
                move || {
                    let mut state = timers.state.lock();
                    loop {
                        if state.shutdown {
                            break;
                        }
                        if let Some(next_due) = state.heap.peek().map(|entry| entry.due) {
                            let now = Instant::now();
                            if next_due <= now {
                                if let Some(entry) = state.heap.pop() {
                                    drop(state);
                                    let location = entry.runnable.metadata().location;
                                    let spawned = entry.runnable.metadata().spawned;
                                    profiler::update_running_task(spawned, location);
                                    entry.runnable.run();
                                    profiler::save_task_timing();
                                    state = timers.state.lock();
                                    continue;
                                }
                            } else {
                                timers.condvar.wait_for(&mut state, next_due - now);
                            }
                        } else {
                            timers.condvar.wait(&mut state);
                        }
                    }
                }
            })
            .expect("failed to spawn Android timer thread");
        threads.push(timer_thread);

        Self {
            background_sender,
            main_sender,
            main_receiver: Mutex::new(main_receiver),
            timers,
            waker: Mutex::new(waker),
            main_thread_id: thread::current().id(),
            _threads: threads,
        }
    }

    pub(crate) fn run_ready_main_tasks(&self) -> bool {
        let mut ran_any = false;
        loop {
            let runnable = {
                let mut receiver = self.main_receiver.lock();
                receiver.try_pop()
            };
            let Ok(Some(runnable)) = runnable else {
                break;
            };
            ran_any = true;
            let location = runnable.metadata().location;
            let spawned = runnable.metadata().spawned;
            profiler::update_running_task(spawned, location);
            runnable.run();
            profiler::save_task_timing();
        }
        ran_any
    }

    pub(crate) fn wake(&self) {
        if let Some(waker) = self.waker.lock().as_ref() {
            waker.wake();
        }
    }
}

impl Drop for AndroidDispatcher {
    fn drop(&mut self) {
        let mut state = self.timers.state.lock();
        state.shutdown = true;
        self.timers.condvar.notify_all();
    }
}

impl PlatformDispatcher for AndroidDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread_id
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        self.background_sender
            .send(priority, runnable)
            .unwrap_or_else(|_| panic!("android dispatcher workers are no longer running"));
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, priority: Priority) {
        if let Err(error) = self.main_sender.send(priority, runnable) {
            // The runnable may wrap a !Send future, so forget it rather than dropping on the wrong thread.
            std::mem::forget(error);
            return;
        }
        self.wake();
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let mut state = self.timers.state.lock();
        let seq = state.next_seq;
        state.next_seq += 1;
        state.heap.push(TimerEntry {
            due: Instant::now() + duration,
            seq,
            runnable,
        });
        self.timers.condvar.notify_one();
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        thread::Builder::new()
            .name("AndroidRealtime".to_owned())
            .spawn(f)
            .expect("failed to spawn Android realtime thread");
    }
}

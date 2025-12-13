use std::{
    pin::Pin,
    sync::{Arc, Condvar, Mutex},
    task::*,
};

// use futures::lock::Mutex;
use server::error::*;

// --- Part 1: The Task/Future ---
// A simple Future that completes after being polled a specific number of times.
struct DelayedPrinter {
    remaining_polls: usize,
    message: String,
}

impl Future for DelayedPrinter {
    type Output = ();

    // The core of async execution. The executor calls this.
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.remaining_polls == 0 {
            println!("Future finished : {}", self.message);
            Poll::Ready(())
        } else {
            // 1. We cannot proceed, so we must register the Waker.
            // In a real runtime, this Waker would be registered with the I/O driver
            // or Timer system waiting for a *real* external event.
            // Here, we just print the Waker registration.
            let waker = cx.waker().clone();
            println!(
                "Future pending: {}. Remaining polls: {}",
                self.message, self.remaining_polls
            );

            // 2. We simulate the external event happening later by waking ourselves up.
            // This simulates the kernel/I/O driver calling wake() when data arrives.
            // We use a separate thread for the wakeup to show the cross-thread nature.
            let remaining_polls = self.remaining_polls;
            let m = self.message.clone();
            std::thread::spawn(move || {
                // Wait a moment to simulate I/O delay
                println!(
                    "waking up task for polls: {} (Simulated I/O Ready) TASK: {}",
                    remaining_polls, m
                );
                waker.wake();
            });
            
            self.remaining_polls -= 1;

            Poll::Pending
        }
    }
}

// --- Part 2: The Executor (The Event Loop) ---
// This is the single-threaded component that drives the polling.

struct Executor {
    task_queue: Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()>>>>>>,
    condvar: Arc<Condvar>,
}

impl Executor {
    fn new() -> Self {
        Executor {
            task_queue: Arc::new(Mutex::new(Vec::new())),
            condvar: Arc::new(Condvar::new()),
        }
    }

    fn spawn(&self, future: impl Future<Output = ()> + 'static) {
        let mut queue = self.task_queue.lock().unwrap();
        queue.push(Box::pin(future))
    }

    fn run(&self) {
        let queue_clone = self.task_queue.clone();
        let condvar_clone = self.condvar.clone();

        let raw_waker = RawWaker::new(
            Arc::into_raw(Arc::new((queue_clone, condvar_clone))) as *const (),
            &VTABLE,
        );

        let waker = unsafe { Waker::from_raw(raw_waker) };

        let mut context = Context::from_waker(&waker);

        loop {
            let mut queue = self.task_queue.lock().unwrap();

            if queue.is_empty() {
                println!("\n😴 Executor blocked (No ready tasks). Waiting for wake...");
                queue = self.condvar.wait(queue).unwrap(); // Thread blocks here
            }

            let mut pending_tasks = Vec::new();

            while let Some(mut task) = queue.pop() {
                match task.as_mut().poll(&mut context) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        pending_tasks.push(task);
                    }
                }
            }

            queue.append(&mut pending_tasks);

            if queue.is_empty() && pending_tasks.is_empty() {
                println!("\n✨ Executor finished all tasks.");
                break;
            }
        }
    }
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

unsafe fn clone_waker(data: *const ()) -> RawWaker {
    let arc = Arc::from_raw(data as *const (Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()>>>>>>, Condvar));
    let new_arc = Arc::clone(&arc);
    let _ = Arc::into_raw(arc);
    RawWaker::new(Arc::into_raw(new_arc) as *const (), &VTABLE)
}

unsafe fn wake(data: *const ()) {
    let arc = Arc::from_raw(data as *const (Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()>>>>>>, Condvar));
    arc.1.notify_one(); // Condvar notification unblocks the run loop
    // Re-queueing the task logic is handled by the Future itself
    // In a real runtime, the Waker's payload would include the task ID to be re-queued.
}
// Required to implement wake_by_ref and drop_waker, which are simpler in this example.
unsafe fn wake_by_ref(data: *const ()) {
    wake(data);
}
unsafe fn drop_waker(data: *const ()) {
    // Decrement the Arc reference count
    let _ = Arc::from_raw(data as *const (Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()>>>>>>, Condvar));
}

fn main() -> Result<()> {
    let executor = Executor::new();
    executor.spawn(DelayedPrinter { remaining_polls:3, message: "Task A".to_string() });
    executor.spawn(DelayedPrinter { remaining_polls:1, message: "Task B".to_string() });
     executor.spawn(DelayedPrinter { remaining_polls:2, message: "Task C".to_string() });
    
    executor.run();
    
    Ok(())
}

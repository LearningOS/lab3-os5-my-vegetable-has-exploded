//! Implementation of [`TaskManager`]
//!
//! It is only used to manage processes and schedule process based on ready queue.
//! Other CPU process monitoring functions are in Processor.

use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use crate::task::task::Pass;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;

pub struct TaskManager {
    ready_queue: Vec<Arc<TaskControlBlock>>,
}

// YOUR JOB: FIFO->Stride
/// A simple FIFO scheduler.
impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(task);
    }
    //LAB3
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let mut min_pass = Pass(u64::MAX);
        let mut min_index = -1;
        for i in (0..self.ready_queue.len()).rev() {
            let i_pass = self.ready_queue[i].inner_exclusive_access().pass.clone();
            if i_pass < min_pass {
                min_pass = i_pass;
                min_index = i as isize;
            }
        }
        if min_index == -1 {
            None
        } else {
            Some(self.ready_queue.remove(min_index as usize))
        }
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}

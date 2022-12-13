//! Types related to task management & Functions for completely changing TCB

use super::TaskContext;
use super::{pid_alloc, KernelStack, PidHandle};
use crate::config::{BIG_STRIDE, MAX_SYSCALL_NUM, TRAP_CONTEXT};
use crate::mm::{MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use crate::timer::get_time_us;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;

/// Task control block structure
///
/// Directly save the contents that will not change during running
pub struct TaskControlBlock {
    // immutable
    /// Process identifier
    pub pid: PidHandle,
    /// Kernel stack corresponding to PID
    pub kernel_stack: KernelStack,
    // LAB1
    pub init_time: usize,
    // mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

// LAB3
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pass(pub u64);

impl Pass {
    pub fn stride(&mut self, prio: u64) {
        self.0 += BIG_STRIDE / prio;
    }
}

/// Structure containing more process content
///
/// Store the contents that will change during operation
/// and are wrapped by UPSafeCell to provide mutual exclusion
pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_cx_ppn: PhysPageNum,
    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    pub base_size: usize,
    /// Save task context
    pub task_cx: TaskContext,
    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,
    /// Application address space
    pub memory_set: MemorySet,
    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    pub parent: Option<Weak<TaskControlBlock>>,
    /// A vector containing TCBs of all child processes of the current process
    pub children: Vec<Arc<TaskControlBlock>>,
    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    //LAB1 syscall record
    pub syscall_counter: Vec<u32>,
    //LAB3
    pub pass: Pass,
    pub prio: u64,
}

/// Simple access to its internal fields
impl TaskControlBlockInner {
    /*
    pub fn get_task_cx_ptr2(&self) -> *const usize {
        &self.task_cx_ptr as *const usize
    }
    */
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }

    //LAB1
    pub fn record_syscall(&mut self, syscall_id: usize) {
        self.syscall_counter[syscall_id] += 1;
    }

    pub fn get_record_syscall(&mut self) -> Vec<u32> {
        self.syscall_counter.clone()
    }

    //LAB2
    pub fn mmap(&mut self, virt_start: VirtAddr, virt_end: VirtAddr, port: usize) -> isize {
        self.memory_set.mmap(virt_start, virt_end, port)
    }

    pub fn unmap(&mut self, virt_start: VirtAddr, virt_end: VirtAddr) -> isize {
        self.memory_set.unmap(virt_start, virt_end)
    }
    //LAB3
    pub fn set_priority(&mut self, priority: isize) {
        self.prio = priority as u64;
    }
}

impl TaskControlBlock {
    /// Get the mutex to get the RefMut TaskControlBlockInner
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// Create a new process
    ///
    /// At present, it is only used for the creation of initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        // push a task context which goes to trap_return to the top of kernel stack
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            init_time: get_time_us() / 1_000,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    syscall_counter: vec![0; MAX_SYSCALL_NUM],
                    pass: Pass(0),
                    prio: 16,
                })
            },
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    // LAB3
    /// Load a new elf and return a tcb then start execution
    pub fn spawn(self: &Arc<TaskControlBlock>, elf_data: &[u8]) -> Arc<TaskControlBlock> {
        let mut parent_inner = self.inner_exclusive_access();
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            init_time: get_time_us() / 1_000,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    syscall_counter: vec![0; MAX_SYSCALL_NUM],
                    pass: Pass(0),
                    prio: 16,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // initialize trap_cx
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        // **** release inner automatically
        // return
        task_control_block
    }
    /// Load a new elf to replace the original application address space and start execution
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();

        // **** access inner exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release inner automatically
    }
    /// Fork from parent to child
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            init_time: get_time_us() / 1_000,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    syscall_counter: vec![0; MAX_SYSCALL_NUM],
                    pass: Pass(0),
                    prio: 16,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access children PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // ---- release parent PCB automatically
        // **** release children PCB automatically
    }
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    pub fn get_start_time(&self) -> usize {
        return self.init_time;
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    UnInit,
    Ready,
    Running,
    Zombie,
}

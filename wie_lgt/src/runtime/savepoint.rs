use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

use spin::Mutex;
use wie_core_arm::{Allocator, ArmCore, ArmCoreContext, ThreadId};
use wie_util::{ByteWrite, Result, WieError};

const SAVE_POINT_SIZE: u32 = 0x10c;

#[derive(Clone)]
struct SavePoint {
    address: u32,
    continuation: Option<ArmCoreContext>,
}

#[derive(Clone, Default)]
pub struct SavePointState {
    chains: Arc<Mutex<BTreeMap<ThreadId, Vec<SavePoint>>>>,
}

impl SavePointState {
    /// Native loader execution happens before `ArmCoreThreadWrapper` exists.
    /// Reserve id zero for that bootstrap execution environment; real WIE
    /// native threads start at id one.
    fn thread_id(core: &ArmCore) -> ThreadId {
        core.current_thread_id().unwrap_or(0)
    }

    pub fn alloc(&self, core: &mut ArmCore, depth: u32) -> Result<u32> {
        let thread_id = Self::thread_id(core);
        let depth = depth as usize;

        {
            let chains = self.chains.lock();
            let len = chains.get(&thread_id).map_or(0, Vec::len);
            if depth > len {
                return Err(WieError::FatalError(alloc::format!(
                    "vm_alloc_save_point depth {depth} exceeds thread {thread_id} chain length {len}"
                )));
            }
        }

        let address = Allocator::alloc(core, SAVE_POINT_SIZE)?;
        core.write_bytes(address, &[0; SAVE_POINT_SIZE as usize])?;

        let mut chains = self.chains.lock();
        let chain = chains.entry(thread_id).or_default();
        chain.insert(
            depth,
            SavePoint {
                address,
                continuation: None,
            },
        );

        Ok(address)
    }

    pub fn capture(&self, core: &ArmCore, address: u32, return_pc: u32) -> Result<()> {
        let thread_id = Self::thread_id(core);
        let mut chains = self.chains.lock();
        let chain = chains.get_mut(&thread_id).ok_or_else(|| {
            WieError::FatalError(alloc::format!(
                "setjmp({address:#x}) has no save-point chain for thread {thread_id}"
            ))
        })?;

        let point = chain.iter_mut().find(|point| point.address == address).ok_or_else(|| {
            WieError::FatalError(alloc::format!(
                "setjmp({address:#x}) does not name a live save point for thread {thread_id}"
            ))
        })?;

        // At the SVC trap the stub has already restored SP/R4. LR is the
        // compiled caller's continuation, whereas PC still names the SVC
        // stub. setjmp must later resume at that caller continuation.
        let mut context = core.save_context();
        context.pc = return_pc;
        if return_pc & 1 != 0 {
            context.cpsr |= 0x20;
        } else {
            context.cpsr &= !0x20;
        }

        point.continuation = Some(context);
        Ok(())
    }

    fn remove(&self, core: &mut ArmCore, depth: u32) -> Result<SavePoint> {
        let thread_id = Self::thread_id(core);
        let depth = depth as usize;

        let point = {
            let mut chains = self.chains.lock();
            let chain = chains.get_mut(&thread_id).ok_or_else(|| {
                WieError::FatalError(alloc::format!(
                    "vm_free_save_point({depth}) has no chain for thread {thread_id}"
                ))
            })?;

            if depth >= chain.len() {
                return Err(WieError::FatalError(alloc::format!(
                    "vm_free_save_point depth {depth} exceeds thread {thread_id} chain length {}",
                    chain.len()
                )));
            }

            let point = chain.remove(depth);
            if chain.is_empty() {
                chains.remove(&thread_id);
            }
            point
        };

        Allocator::free(core, point.address, SAVE_POINT_SIZE)?;
        Ok(point)
    }

    pub fn free(&self, core: &mut ArmCore, depth: u32) -> Result<u32> {
        Ok(self.remove(core, depth)?.address)
    }

    /// Native `vm_throw_exception` pops depth zero and calls
    /// `longjmp(save_point, exception)`. Restore the context captured by
    /// table 1 / function 0x32 and make setjmp appear to return `exception`.
    pub fn throw(&self, core: &mut ArmCore, exception: u32) -> Result<()> {
        let point = self.remove(core, 0)?;
        let context = point.continuation.ok_or_else(|| {
            WieError::FatalError(alloc::format!(
                "longjmp target {:#x} has no captured setjmp continuation",
                point.address
            ))
        })?;

        let return_pc = context.pc;
        core.restore_context(&context);
        core.write_return_value(&[exception])?;
        core.set_next_pc(return_pc)?;

        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use wie_core_arm::{Allocator, ArmCore};

    use super::SavePointState;

    fn core() -> ArmCore {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        core
    }

    #[test]
    fn depth_selects_native_chain_position() {
        let mut core = core();
        let state = SavePointState::default();

        let head = state.alloc(&mut core, 0).unwrap();
        let tail = state.alloc(&mut core, 1).unwrap();
        let middle = state.alloc(&mut core, 1).unwrap();

        // Chain is now [head, middle, tail].
        assert_eq!(state.free(&mut core, 1).unwrap(), middle);
        assert_eq!(state.free(&mut core, 1).unwrap(), tail);
        assert_eq!(state.free(&mut core, 0).unwrap(), head);
    }

    #[test]
    fn throw_restores_setjmp_context_and_returns_exception_in_r0() {
        let mut core = core();
        let state = SavePointState::default();

        let save_point = state.alloc(&mut core, 0).unwrap();

        let mut before = core.save_context();
        before.r0 = save_point;
        before.r4 = 0x4444_4444;
        before.r7 = 0x7777_7777;
        before.sp = 0x4000_1000;
        before.lr = 0x1234_5678;
        before.pc = 0x2000_0001;
        before.cpsr |= 0x20;
        core.restore_context(&before);

        let continuation = 0x3000_0001;
        state.capture(&core, save_point, continuation).unwrap();

        // Destroy the live register state after setjmp.
        let mut changed = core.save_context();
        changed.r0 = 0;
        changed.r4 = 0;
        changed.r7 = 0;
        changed.sp = 0x4000_2000;
        changed.lr = 0;
        changed.pc = 0x5000_0001;
        core.restore_context(&changed);

        let exception = 0x6abc_def0;
        state.throw(&mut core, exception).unwrap();

        let after = core.save_context();
        assert_eq!(after.r0, exception);
        assert_eq!(after.r4, 0x4444_4444);
        assert_eq!(after.r7, 0x7777_7777);
        assert_eq!(after.sp, 0x4000_1000);
        assert_eq!(after.lr, 0x1234_5678);
        assert_eq!(after.pc, continuation & !1);
        assert_ne!(after.cpsr & 0x20, 0);
    }

    #[test]
    fn invalid_depth_is_rejected_without_mutating_chain() {
        let mut core = core();
        let state = SavePointState::default();

        let head = state.alloc(&mut core, 0).unwrap();
        assert!(state.alloc(&mut core, 2).is_err());
        assert!(state.free(&mut core, 1).is_err());
        assert_eq!(state.free(&mut core, 0).unwrap(), head);
    }
}

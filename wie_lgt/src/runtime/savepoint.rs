use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

use spin::Mutex;
use wie_core_arm::{Allocator, ArmCore, ArmCoreContext, ThreadId};
use wie_util::{ByteWrite, Result, WieError, read_generic};

const SAVE_POINT_SIZE: u32 = 0x10c;

/// Names one call into compiled code, so its save points can be discarded when
/// it returns. Held by the caller for the length of the call.
#[derive(Clone, Copy)]
pub struct FrameToken {
    thread_id: ThreadId,
    /// How many frames were live once this one began, so a token cannot end a
    /// frame that is not the innermost.
    depth: usize,
}

/// A real save-point chain is a handful of frames deep. A "depth" far above this
/// is not a chain index at all - on some titles the import slot that resolves to
/// `vm_alloc_save_point` here (table `0x64` function `0x03`) is a different
/// function whose pointer/size argument lands in the same register. Rather than
/// end the run on it - which hides everything the game does next, the very thing
/// that reveals what the slot really is - such a call is logged and allowed to
/// continue with an appended save point.
const IMPLAUSIBLE_SAVE_POINT_DEPTH: usize = 0x1000;

#[derive(Clone)]
struct SavePoint {
    address: u32,
    continuation: Option<ArmCoreContext>,
}

/// One thread's save points, newest first, and the frame boundaries laid across
/// them.
///
/// A save point is a place in the ARM stack to longjmp back to, so it is only
/// good while the ARM call that made it is still running. Each entry into
/// compiled code records how many save points already existed
/// (`SavePointState::enter_frame`); everything allocated past that mark belongs
/// to that call and dies with it.
#[derive(Clone, Default)]
struct Chain {
    points: Vec<SavePoint>,
    /// Save-point counts at each live compiled call's entry, outermost first.
    frames: Vec<usize>,
}

impl Chain {
    /// Save points the innermost compiled call owns - the ones a `longjmp` can
    /// still reach, since they name frames of the ARM call that is running now.
    fn reachable(&self) -> usize {
        self.points.len() - self.frames.last().copied().unwrap_or(0)
    }
}

#[derive(Clone, Default)]
pub struct SavePointState {
    chains: Arc<Mutex<BTreeMap<ThreadId, Chain>>>,
}

impl SavePointState {
    /// The collector roots every live save point contributes: the register
    /// words a longjmp would restore, and the guest jmp_buf blocks themselves.
    ///
    /// A save point outlives the moment it was captured, so a callee-saved
    /// register it holds can be the last reference to an object the frame's
    /// *current* registers no longer name - and the sweep would free it.
    pub fn gc_roots(&self) -> (Vec<u32>, Vec<(u32, u32)>) {
        let chains = self.chains.lock();

        let mut registers = Vec::new();
        let mut ranges = Vec::new();

        for chain in chains.values() {
            for point in &chain.points {
                ranges.push((point.address, point.address + SAVE_POINT_SIZE));

                if let Some(context) = &point.continuation {
                    registers.extend_from_slice(&[
                        context.r0, context.r1, context.r2, context.r3, context.r4, context.r5, context.r6, context.r7, context.r8, context.sb,
                        context.sl, context.fp, context.ip, context.lr,
                    ]);
                }
            }
        }

        (registers, ranges)
    }

    /// Native loader execution happens before `ArmCoreThreadWrapper` exists.
    /// Reserve id zero for that bootstrap execution environment; real WIE
    /// native threads start at id one.
    fn thread_id(core: &ArmCore) -> ThreadId {
        core.current_thread_id().unwrap_or(0)
    }

    /// Marks the start of a call into compiled code. The returned token is
    /// handed back to `leave_frame` when the call returns, however it returns.
    pub fn enter_frame(&self, core: &ArmCore) -> FrameToken {
        let thread_id = Self::thread_id(core);
        let mut chains = self.chains.lock();
        let chain = chains.entry(thread_id).or_default();

        chain.frames.push(chain.points.len());

        FrameToken {
            thread_id,
            depth: chain.frames.len(),
        }
    }

    /// Ends the call `enter_frame` began, discarding any save point it left
    /// behind.
    ///
    /// A compiled method that returns through a catch block does not always
    /// free the save point of the `try` it left - and once the call has
    /// returned, the ARM stack that save point named is gone. Left in the chain
    /// it becomes the target of the next `throw`, which then resumes in a frame
    /// that no longer exists; that is how a title dies on an
    /// out-of-bounds index it would otherwise have caught, seconds after the
    /// method that leaked the save point returned.
    pub fn leave_frame(&self, core: &mut ArmCore, token: FrameToken) {
        let abandoned = {
            let mut chains = self.chains.lock();
            let Some(chain) = chains.get_mut(&token.thread_id) else {
                return;
            };

            // Frames unwind in order; a token from a call that already ended (or
            // from another chain) names nothing to clean up.
            if chain.frames.len() != token.depth {
                return;
            }

            let mark = chain.frames.pop().unwrap_or(0);
            let abandoned: Vec<_> = chain.points.drain(..chain.points.len().saturating_sub(mark)).collect();
            if chain.points.is_empty() && chain.frames.is_empty() {
                chains.remove(&token.thread_id);
            }
            abandoned
        };

        if !abandoned.is_empty() {
            tracing::debug!("Discarding {} save point(s) left behind by a compiled call", abandoned.len());
        }
        for point in abandoned {
            if let Err(error) = Allocator::free(core, point.address, SAVE_POINT_SIZE) {
                tracing::error!("Failed to free abandoned save point {:#x}: {error}", point.address);
            }
        }
    }

    pub fn alloc(&self, core: &mut ArmCore, depth: u32) -> Result<u32> {
        let thread_id = Self::thread_id(core);
        let raw_depth = depth;
        let depth = depth as usize;

        let insert_at = {
            let chains = self.chains.lock();
            let len = chains.get(&thread_id).map_or(0, Chain::reachable);

            if depth > IMPLAUSIBLE_SAVE_POINT_DEPTH {
                // Not a chain index - dump what the game actually passed so the
                // slot's real meaning can be identified, then append and carry on.
                let mut words = alloc::string::String::new();
                for i in 0..8u32 {
                    match read_generic::<u32, _>(core, raw_depth.wrapping_add(i * 4)) {
                        Ok(value) => words.push_str(&alloc::format!(" {value:#010x}")),
                        Err(_) => {
                            words.push_str(" <unmapped>");
                            break;
                        }
                    }
                }
                tracing::warn!(
                    "vm_alloc_save_point depth {raw_depth:#x} is a pointer, not a chain index (thread {thread_id}, chain len {len}); \
                     slot (0x64,0x03) may not be vm_alloc_save_point on this title. Appending and continuing. [{raw_depth:#x}]:{words}"
                );
                len
            } else if depth > len {
                return Err(WieError::FatalError(alloc::format!(
                    "vm_alloc_save_point depth {depth} exceeds thread {thread_id} chain length {len}"
                )));
            } else {
                depth
            }
        };

        let address = Allocator::alloc(core, SAVE_POINT_SIZE)?;
        core.write_bytes(address, &[0; SAVE_POINT_SIZE as usize])?;

        let mut chains = self.chains.lock();
        let chain = chains.entry(thread_id).or_default();
        // A save point belongs to the call that made it, so it can never be
        // placed below the frames of the calls this one was made from.
        let insert_at = insert_at.min(chain.reachable());
        chain.points.insert(insert_at, SavePoint { address, continuation: None });

        Ok(address)
    }

    pub fn capture(&self, core: &ArmCore, address: u32, return_pc: u32) -> Result<()> {
        let thread_id = Self::thread_id(core);
        let mut chains = self.chains.lock();
        let chain = chains
            .get_mut(&thread_id)
            .ok_or_else(|| WieError::FatalError(alloc::format!("setjmp({address:#x}) has no save-point chain for thread {thread_id}")))?;

        let point = chain.points.iter_mut().find(|point| point.address == address).ok_or_else(|| {
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
            let chain = chains
                .get_mut(&thread_id)
                .ok_or_else(|| WieError::FatalError(alloc::format!("vm_free_save_point({depth}) has no chain for thread {thread_id}")))?;

            if depth >= chain.reachable() {
                return Err(WieError::FatalError(alloc::format!(
                    "vm_free_save_point depth {depth} exceeds thread {thread_id} chain length {}",
                    chain.reachable()
                )));
            }

            let point = chain.points.remove(depth);
            if chain.points.is_empty() && chain.frames.is_empty() {
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
        let thread_id = Self::thread_id(core);

        // No save point this call can reach means the compiled code has no
        // try/catch around the throw (nor the reference firmware's top-level
        // handler, which WIE does not run as guest code) - a save point an outer
        // call left behind names an ARM frame this one cannot return into, so it
        // is not one either. The exception is uncaught in compiled code: hand it
        // to the JVM as a real Java exception so it propagates with a stack
        // trace and a Java catch higher up can still handle it, rather than
        // resuming in a frame that is gone.
        let has_point = self.chains.lock().get(&thread_id).is_some_and(|chain| chain.reachable() > 0);
        if !has_point {
            return Err(WieError::JavaException(exception));
        }

        let point = self.remove(core, 0)?;
        let Some(context) = point.continuation else {
            // Allocated but never captured by setjmp, so there is nowhere to
            // longjmp; treat it as uncaught and propagate the same way.
            return Err(WieError::JavaException(exception));
        };

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
    use wie_util::WieError;

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
    fn a_call_takes_its_leftover_save_points_with_it() {
        let mut core = core();
        let state = SavePointState::default();

        // A compiled call that returns through a catch block does not always
        // free the save point of the `try` it left. It names a frame of an ARM
        // stack that is gone once the call returns, so it must not survive it.
        let frame = state.enter_frame(&core);
        let leaked = state.alloc(&mut core, 0).unwrap();
        state.capture(&core, leaked, 0x2000_0001).unwrap();
        state.leave_frame(&mut core, frame);

        // The next call's throw finds nothing to longjmp to and hands the
        // exception to the JVM instead of resuming in the frame that is gone.
        let frame = state.enter_frame(&core);
        assert!(matches!(state.throw(&mut core, 0x6abc_def0), Err(WieError::JavaException(0x6abc_def0))));
        state.leave_frame(&mut core, frame);
    }

    #[test]
    fn a_call_cannot_throw_to_the_call_that_made_it() {
        let mut core = core();
        let state = SavePointState::default();

        let outer = state.enter_frame(&core);
        let outer_point = state.alloc(&mut core, 0).unwrap();
        state.capture(&core, outer_point, 0x2000_0001).unwrap();

        // A nested call sees an empty chain of its own: the outer save point
        // belongs to an ARM frame it cannot return into.
        let inner = state.enter_frame(&core);
        assert!(matches!(state.throw(&mut core, 0x1111_2222), Err(WieError::JavaException(0x1111_2222))));
        state.leave_frame(&mut core, inner);

        // ... and the outer call still has its own.
        assert!(state.throw(&mut core, 0x3333_4444).is_ok());
        state.leave_frame(&mut core, outer);
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
    fn pointer_sized_depth_is_appended_rather_than_fatal() {
        // A title whose (0x64,0x03) slot is not vm_alloc_save_point passes a
        // pointer here. It must not end the run: the call appends a save point
        // and returns it, so the game runs on and reveals the slot's real use.
        let mut core = core();
        let state = SavePointState::default();

        let head = state.alloc(&mut core, 0).unwrap();
        let bogus = state.alloc(&mut core, 0x735d4).unwrap();
        assert_ne!(bogus, 0);

        // The bogus call landed at the end, leaving the real head reachable.
        assert_eq!(state.free(&mut core, 1).unwrap(), bogus);
        assert_eq!(state.free(&mut core, 0).unwrap(), head);
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

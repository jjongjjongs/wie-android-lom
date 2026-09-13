//! A bounded control-flow trace, armed from the emulator and read out of a log.
//!
//! Some things a title does cannot be learned from a log of what it asked the
//! platform for, because the answer it was waiting on never existed. A billing
//! reply is the standing example: the gateway these titles talk to has been off
//! for years, so a capture of one of their purchases holds our own stand-in
//! answer and nothing else. What the title would have accepted is written only
//! in its own code, in the branch it takes after reading the reply.
//!
//! This makes that branch visible. Arming the probe records where the ARM core
//! goes next whenever it does not simply fall through to the following
//! instruction, and writes those edges to the log; read against the title's own
//! `binary.mod`, a trace is the parse itself - every compare, and which way each
//! one went. One run on a handset then says what a reply has to look like,
//! where a log alone can only say that the one we sent was wrong.
//!
//! Edges rather than instructions, because a trace has to survive a handset's
//! log buffer: straight-line code is most of what runs and none of what a parse
//! is decided by, and a tight loop is a single edge repeated, which is recorded
//! as one entry and a count. What is left is small enough to read.
//!
//! It is off unless something arms it, and arming is deliberate and bounded:
//! the count is spent and the probe goes quiet again. The cost while it sleeps
//! is one relaxed load per instruction, which the interpreter's loop already
//! pays several of.

use alloc::{format, string::String, vec::Vec};

use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

/// Edges still to record. Zero - the resting state - is what the core checks,
/// so a sleeping probe costs one relaxed load.
static REMAINING: AtomicU32 = AtomicU32::new(0);

/// Where the core was when it was last looked at, so the next look can tell a
/// fall-through from a jump.
static LAST: AtomicU32 = AtomicU32::new(0);

/// Recorded edges not yet written out, each with how many times it repeated.
static PENDING: Mutex<Vec<Edge>> = Mutex::new(Vec::new());

/// What armed the probe, carried into the trace lines so a log holding several
/// traces says which is which.
static LABEL: Mutex<Option<String>> = Mutex::new(None);

/// A trace to start once the answer it is about has been read, rather than when
/// it was written. See [`arm_when_drained`].
static WHEN_DRAINED: Mutex<Option<(String, u32)>> = Mutex::new(None);

/// Edges per trace line. A line is for reading; this keeps one to about the
/// width of a terminal.
const BATCH: usize = 12;

/// How far back an edge is looked for before it counts as a new one.
///
/// A loop closes within its own length, so this is what a loop may be and still
/// be counted rather than written out over and over. Past it an edge is taken
/// as somewhere the title has arrived afresh, which is what keeps a parse
/// reading in the order it happened.
const CYCLE: usize = 8;

/// One place the core jumped from and to, and how many times in a row it made
/// that same jump - which is what a loop looks like from here.
#[derive(Clone, Copy, Eq, PartialEq)]
struct Edge {
    from: u32,
    to: u32,
    times: u32,
}

/// Starts recording the next `count` edges, under `label`.
///
/// Arming again while a trace is running replaces it: the interesting run is
/// the one that was just armed, and the tail of a spent trace is noise.
pub fn arm(label: &str, count: u32) {
    flush();

    LAST.store(0, Ordering::Relaxed);
    *LABEL.lock() = Some(String::from(label));
    REMAINING.store(count, Ordering::Relaxed);

    tracing::info!("probe: tracing the next {count} branches for {label}");
}

/// Arms a trace for the moment a queued answer has been read rather than the
/// moment it was written.
///
/// A title reads a reply some way after the gateway put it there - it takes the
/// length first and the body on a later call, and the scheduler runs between
/// them. Tracing from the write would spend the count on that wait; tracing
/// from the last byte leaving the gateway starts it on the parse, which is what
/// the trace is for.
pub fn arm_when_drained(label: &str, count: u32) {
    *WHEN_DRAINED.lock() = Some((String::from(label), count));
}

/// Starts a trace [`arm_when_drained`] queued, if one is waiting.
///
/// Call where a queued answer runs out, which is where the title has all of it
/// and nothing is left to wait for.
pub fn drained() {
    let queued = WHEN_DRAINED.lock().take();

    let Some((label, count)) = queued else {
        return;
    };

    // A trace already running is one someone is waiting on the whole of.
    // Replacing it here would leave two half-traces where a caller asked for
    // one, so the newcomer gives way.
    if is_armed() {
        tracing::info!("probe: {label} was not traced; a trace was already running");
        return;
    }

    arm(&label, count);
}

/// Program counters a trace can be started at, or zero for a free slot. Kept as
/// plain atomics because the core reads them once per instruction; everything
/// else about a watch lives in [`WATCHES`], which is only touched on a hit.
static WATCH_PCS: [AtomicU32; WATCH_SLOTS] = [const { AtomicU32::new(0) }; WATCH_SLOTS];

/// How many of [`WATCH_PCS`] are set. The one load the core makes when no trace
/// is running.
static WATCH_LIVE: AtomicU32 = AtomicU32::new(0);

/// What each watched address is and how much to record from it.
static WATCHES: Mutex<[Option<Watch>; WATCH_SLOTS]> = Mutex::new([const { None }; WATCH_SLOTS]);

/// Addresses that can be watched at once.
///
/// A question usually needs two: the routine suspected of bailing, and the one
/// that should have set up what it bails on. More than a handful and the
/// per-instruction scan stops being free.
const WATCH_SLOTS: usize = 4;

/// Times a watched address is traced from before it stops being interesting.
///
/// A draw routine runs every frame. The first few passes say what the rest
/// would, and a trace per frame would bury the log.
const WATCH_TRACE_LIMIT: u32 = 3;

/// One watched address.
#[derive(Clone)]
struct Watch {
    label: String,
    branches: u32,
    hits: u32,
}

/// Starts a trace whenever the core reaches `pc`.
///
/// The other way in - [`arm_when_drained`] - hangs a trace off an answer this
/// emulator gave, which only works where the title asked it something. A screen
/// that draws wrongly asks nothing. This hangs one off an address instead, so a
/// routine read out of the title's own `binary.mod` can be watched directly.
///
/// **A watch that never fires is itself the answer.** A routine suspected of
/// bailing early may not be reached at all, and the two look identical from
/// outside; a log with no trace in it says which. That is why these come in
/// twos - one address alone cannot tell "not reached" from "reached and
/// declined somewhere earlier".
///
/// Compiled blocks are entered at their first instruction, so an address that
/// starts a function or is a branch target is seen whichever engine is running.
/// One in the middle of a straight run is only seen by the interpreter.
pub fn watch(pc: u32, label: &str, branches: u32) {
    let mut watches = WATCHES.lock();

    let Some(slot) = watches.iter().position(Option::is_none) else {
        tracing::warn!("probe: no room to watch {pc:#x} for {label}");
        return;
    };

    watches[slot] = Some(Watch {
        label: String::from(label),
        branches,
        hits: 0,
    });
    drop(watches);

    WATCH_PCS[slot].store(pc, Ordering::Relaxed);
    WATCH_LIVE.fetch_add(1, Ordering::Relaxed);

    tracing::info!("probe: watching {pc:#x} for {label}");
}

/// Whether any address is being watched. One relaxed load, called per
/// instruction while no trace is running.
#[inline(always)]
pub fn is_watching() -> bool {
    WATCH_LIVE.load(Ordering::Relaxed) != 0
}

/// Starts a trace if `pc` is one of the watched addresses.
///
/// Call once per instruction under [`is_watching`], and only while no trace is
/// running - a trace that reaches a watched address again should carry on
/// rather than restart.
pub fn reached(pc: u32) {
    let Some(slot) = WATCH_PCS.iter().position(|watched| watched.load(Ordering::Relaxed) == pc) else {
        return;
    };

    let mut watches = WATCHES.lock();
    let Some(watch) = watches[slot].as_mut() else {
        return;
    };

    watch.hits += 1;
    let (hits, label, branches) = (watch.hits, watch.label.clone(), watch.branches);

    if hits > WATCH_TRACE_LIMIT {
        // Reached again, and said what it had to say. Give the slot up, so the
        // per-instruction scan shortens and eventually costs nothing.
        watches[slot] = None;
        drop(watches);

        WATCH_PCS[slot].store(0, Ordering::Relaxed);
        WATCH_LIVE.fetch_sub(1, Ordering::Relaxed);

        tracing::info!("probe: {pc:#x} was reached {hits} times; no longer watching it");
        return;
    }
    drop(watches);

    arm(&alloc::format!("{label} #{hits}"), branches);
}

/// Addresses whose next write should be reported, with what wrote it.
static WRITE_WATCHES: Mutex<Vec<WriteWatch>> = Mutex::new(Vec::new());

/// Whether any address is write-watched. The one load the memory path makes.
static WRITE_LIVE: AtomicU32 = AtomicU32::new(0);

/// Where the core is, published each instruction while a write watch is live so
/// a write can say what wrote it.
static WRITING_PC: AtomicU32 = AtomicU32::new(0);

/// Writes reported per address before it stops being interesting. A value that
/// is rewritten every frame would otherwise fill the log with the same line.
const WRITE_REPORT_LIMIT: u32 = 8;

/// One write-watched address.
struct WriteWatch {
    address: u32,
    label: String,
    reports: u32,
}

/// Reports the next few writes to `address`, and what the core was executing
/// when each happened.
///
/// The question this answers - "who set this, and to what" - is the one that
/// static reading is worst at. A value can be written through a pointer that
/// arrived in a register from three calls away, and no scan of the instruction
/// stream finds that; the write itself is unmissable.
///
/// **Silence is an answer here too**, and often the useful one: a field the
/// title reads and nothing ever writes is a field whose code path never runs.
pub fn watch_write(address: u32, label: &str) {
    let mut watches = WRITE_WATCHES.lock();

    if watches.iter().any(|watch| watch.address == address) {
        return;
    }

    watches.push(WriteWatch {
        address,
        label: String::from(label),
        reports: 0,
    });
    let live = watches.len();
    drop(watches);

    WRITE_LIVE.store(live as u32, Ordering::Relaxed);

    tracing::info!("probe: watching writes to {address:#x} for {label}");
}

/// Whether any address is write-watched. One relaxed load, called on the memory
/// path and once per instruction.
#[inline(always)]
pub fn is_write_watching() -> bool {
    WRITE_LIVE.load(Ordering::Relaxed) != 0
}

/// Records where the core is, so a write can name what made it.
///
/// Call once per instruction under [`is_write_watching`].
#[inline(always)]
pub fn at(pc: u32) {
    WRITING_PC.store(pc, Ordering::Relaxed);
}

/// Reports a write of `value` (`width` bytes) at `address`, if it is watched.
///
/// Call from the memory path under [`is_write_watching`]. A write of any width
/// that touches a watched address counts, so a byte store into the middle of a
/// watched word is not missed.
pub fn wrote(address: u32, width: u32, value: u32) {
    let mut watches = WRITE_WATCHES.lock();

    let Some(watch) = watches
        .iter_mut()
        .find(|watch| watch.address >= address && watch.address < address.saturating_add(width))
    else {
        return;
    };

    watch.reports += 1;
    if watch.reports > WRITE_REPORT_LIMIT {
        return;
    }

    let (target, label, reports) = (watch.address, watch.label.clone(), watch.reports);
    let last = reports == WRITE_REPORT_LIMIT;
    drop(watches);

    let pc = WRITING_PC.load(Ordering::Relaxed);
    tracing::info!("probe: {target:#x} written {value:#x} ({width} bytes at {address:#x}) from {pc:#x} - {label} #{reports}");

    if last {
        tracing::info!("probe: {target:#x} has been written {reports} times; no longer reporting it");
    }
}

/// Reads a watch specification and arms what it names.
///
/// The whole point of taking this as text at runtime is that a new question
/// stops needing a new build: any address in any title can be watched from the
/// handset, the same way the log filter can be changed there.
///
/// Comma-separated, each entry one of:
///
/// ```text
/// pc:3a376        trace from this address, for the default number of branches
/// pc:3a376/50000  the same, for that many branches
/// w:1518700       report the next writes to this address
/// ```
///
/// Addresses are hexadecimal, with or without `0x`. An empty specification
/// clears every watch. Returns the reason an entry was rejected.
pub fn set_watches(spec: &str) -> Result<(), String> {
    clear_watches();

    for entry in spec.split(',').map(str::trim).filter(|entry| !entry.is_empty()) {
        let (kind, rest) = entry
            .split_once(':')
            .ok_or_else(|| alloc::format!("{entry:?} is not `pc:<addr>` or `w:<addr>`"))?;

        let (address, branches) = match rest.split_once('/') {
            Some((address, branches)) => (
                address,
                Some(
                    branches
                        .parse::<u32>()
                        .map_err(|_| alloc::format!("{branches:?} is not a number of branches"))?,
                ),
            ),
            None => (rest, None),
        };

        let address = address.trim().trim_start_matches("0x");
        let address = u32::from_str_radix(address, 16).map_err(|_| alloc::format!("{address:?} is not a hexadecimal address"))?;

        match kind.trim() {
            "pc" => watch(address, entry, branches.unwrap_or(DEFAULT_WATCH_BRANCHES)),
            "w" => watch_write(address, entry),
            other => return Err(alloc::format!("{other:?} is not `pc` or `w`")),
        }
    }

    Ok(())
}

/// Branches recorded from a `pc:` watch that does not say how many.
const DEFAULT_WATCH_BRANCHES: u32 = 30_000;

/// Forgets every watch, leaving the per-instruction checks costing nothing.
pub fn clear_watches() {
    for pc in &WATCH_PCS {
        pc.store(0, Ordering::Relaxed);
    }
    WATCH_LIVE.store(0, Ordering::Relaxed);
    *WATCHES.lock() = [const { None }; WATCH_SLOTS];

    WRITE_WATCHES.lock().clear();
    WRITE_LIVE.store(0, Ordering::Relaxed);
}

/// What is being watched, for the UI to show.
pub fn watches() -> String {
    let mut parts: Vec<String> = WATCHES
        .lock()
        .iter()
        .flatten()
        .map(|watch| alloc::format!("pc:{}", watch.label))
        .collect();

    parts.extend(WRITE_WATCHES.lock().iter().map(|watch| alloc::format!("w:{:x}", watch.address)));

    parts.join(",")
}

/// Stops any trace, running or queued, and forgets what it had recorded./// Stops any trace, running or queued, and forgets what it had recorded.
///
/// The probe is one thing shared by the process. A caller that has finished
/// with it - or a test that must not leave one armed behind it - puts it back
/// to rest here.
pub fn disarm() {
    REMAINING.store(0, Ordering::Relaxed);
    LAST.store(0, Ordering::Relaxed);
    UNANSWERED_TRACES.store(0, Ordering::Relaxed);
    for pc in &WATCH_PCS {
        pc.store(0, Ordering::Relaxed);
    }
    WATCH_LIVE.store(0, Ordering::Relaxed);
    *WATCHES.lock() = [const { None }; WATCH_SLOTS];
    PENDING.lock().clear();
    *LABEL.lock() = None;
    *WHEN_DRAINED.lock() = None;
}

/// Traces started by [`over_an_unanswered_request`] so far.
static UNANSWERED_TRACES: AtomicU32 = AtomicU32::new(0);

/// How many of those one run is worth.
///
/// A title that gets nothing it recognises usually asks again, and often in a
/// loop; the first few attempts say everything the later ones would, and a
/// trace per attempt would bury the log it is written to.
const UNANSWERED_TRACE_LIMIT: u32 = 4;

/// Branches to record over an unanswered request's parse.
///
/// Enough to carry the read out of the stream, the walk over whatever came
/// back, and the branch that gives up on it.
const UNANSWERED_TRACE_BRANCHES: u32 = 30_000;

/// Queues a trace over what a title does with an answer nobody shaped for it.
///
/// A request no matcher knows is where a title's protocol goes unread, and the
/// stand-in it gets instead is one it was never going to accept. What it does
/// next - which field it reads, which compare it fails, which screen it settles
/// on - is the whole of what a later matcher has to be built from, and none of
/// it is in an ordinary log.
///
/// So arm one here rather than waiting for someone to wire a probe up per
/// title: the first unanswered request of a run carries its own trace, and
/// reading a handset log is enough to start. Bounded to
/// [`UNANSWERED_TRACE_LIMIT`] traces a run, because a title that is not
/// answered tends to ask again.
pub fn over_an_unanswered_request(what: &str) {
    let started = UNANSWERED_TRACES.fetch_add(1, Ordering::Relaxed);
    if started >= UNANSWERED_TRACE_LIMIT {
        UNANSWERED_TRACES.store(UNANSWERED_TRACE_LIMIT, Ordering::Relaxed);
        return;
    }

    arm_when_drained(&format!("답이 없는 요청: {what}"), UNANSWERED_TRACE_BRANCHES);
}

/// Whether the core should be recording. One relaxed load, called per
/// instruction.
#[inline(always)]
pub fn is_armed() -> bool {
    REMAINING.load(Ordering::Relaxed) != 0
}

/// Looks at where the core is, recording an edge if it did not get there by
/// falling through from where it was.
///
/// Call once per instruction under [`is_armed`], before the instruction runs.
pub fn observe(pc: u32) {
    let left = REMAINING.load(Ordering::Relaxed);
    if left == 0 {
        return;
    }

    let last = LAST.swap(pc, Ordering::Relaxed);

    // The first look has nothing to compare against, and an instruction reached
    // from the one before it says nothing about a decision. ARM instructions are
    // four bytes and Thumb two or four, so either step is a fall-through.
    if last == 0 || pc == last + 2 || pc == last + 4 {
        return;
    }

    REMAINING.store(left - 1, Ordering::Relaxed);

    let mut pending = PENDING.lock();

    // A loop is the same few edges over and over, and only the tightest of them
    // is the same edge twice in a row: a loop with a branch in it alternates
    // between two edges and a longer one cycles through several, so counting
    // only the last would write a line per turn of exactly the loops worth
    // collapsing. Look back over the batch instead - far enough to close a
    // cycle, near enough to leave a parse's own order intact.
    if let Some(previous) = pending
        .iter_mut()
        .rev()
        .take(CYCLE)
        .find(|previous| previous.from == last && previous.to == pc)
    {
        previous.times += 1;
        return;
    }

    pending.push(Edge {
        from: last,
        to: pc,
        times: 1,
    });

    if pending.len() < BATCH && left > 1 {
        return;
    }

    let line = trace_line(&pending);
    pending.clear();
    drop(pending);

    tracing::info!("{line}");

    // Say where it stopped. A trace that runs out mid-way and says nothing
    // reads exactly like a title that stopped branching, which is the wrong
    // conclusion to leave lying around.
    if left == 1 {
        let label = LABEL.lock().clone().unwrap_or_default();
        tracing::info!("probe: {label} recorded all it was given and stopped here");
    }
}

/// Writes out a partly filled batch, so the tail of a trace is not lost.
pub fn flush() {
    let mut pending = PENDING.lock();
    if pending.is_empty() {
        return;
    }

    let line = trace_line(&pending);
    pending.clear();
    drop(pending);

    tracing::info!("{line}");
}

/// One trace line: the label the probe was armed under, then the edges, each
/// `from>to` and - where it repeated - `*times`.
fn trace_line(edges: &[Edge]) -> String {
    let label = LABEL.lock().clone().unwrap_or_default();
    let mut line = format!("probe {label}:");

    for edge in edges {
        line.push_str(&format!(" {:x}>{:x}", edge.from, edge.to));

        if edge.times > 1 {
            line.push_str(&format!("*{}", edge.times));
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe is one thing shared by the whole process, so two tests driving
    /// it at once would each see the other's edges. Hold this for the length of
    /// any test that arms it.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// The edges a walk over `path` records, as a trace line would read them.
    fn trace_of(path: &[u32]) -> String {
        for pc in path {
            observe(*pc);
        }

        let pending = PENDING.lock().clone();
        trace_line(&pending)
    }

    /// The loop one 레전드오브마스터2 capture spent itself in: two edges taken
    /// in turn, which counting only the last edge wrote out over and over -
    /// thirty thousand lines of it, with three lines of network underneath.
    #[test]
    fn a_loop_that_alternates_is_counted_rather_than_written_out() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 64);

        let mut path = Vec::new();
        for _ in 0..8 {
            path.extend_from_slice(&[0x4a96c, 0x4a8de]);
        }

        let line = trace_of(&path);

        // Two edges carrying their counts, however many turns the loop took.
        assert_eq!(line.matches('>').count(), 2, "{line}");
        assert!(line.contains("4a96c>4a8de*8"), "{line}");
        assert!(line.contains("4a8de>4a96c*7"), "{line}");
    }

    /// Only as far back as a loop can close, so a parse that comes back to an
    /// address much later still reads in the order it happened.
    #[test]
    fn an_edge_taken_again_long_after_is_its_own_entry() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 64);

        // The edge, then more separating it than a loop is looked for over,
        // then the edge again - all inside one batch so nothing is written out
        // before it can be counted.
        let mut path = alloc::vec![0x1000, 0x2000];
        for step in 0..CYCLE {
            path.push(0x3000 + (step as u32) * 0x1000);
        }
        path.extend_from_slice(&[0x1000, 0x2000]);

        let line = trace_of(&path);

        assert_eq!(line.matches("1000>2000").count(), 2, "{line}");
    }

    #[test]
    fn a_probe_that_was_never_armed_is_not_armed() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        assert!(!is_armed());
    }

    #[test]
    fn straight_line_code_is_not_a_branch() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 8);

        // Four-byte ARM, then two-byte Thumb: both are fall-through.
        assert_eq!(trace_of(&[0x1a28, 0x1a2c, 0x1a30, 0x1a32]), "probe test:");
        disarm();
    }

    #[test]
    fn a_jump_is_recorded_as_the_edge_it_took() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 8);

        assert_eq!(trace_of(&[0x1a28, 0x1a2c, 0x520b4]), "probe test: 1a2c>520b4");
        disarm();
    }

    #[test]
    fn a_loop_is_one_edge_and_a_count() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 8);

        let mut path = alloc::vec![];
        for _ in 0..3 {
            path.extend_from_slice(&[0x1000, 0x1004, 0x1008]);
        }

        assert_eq!(trace_of(&path), "probe test: 1008>1000*2");
        disarm();
    }

    #[test]
    fn a_trace_stops_after_the_count_of_branches_it_was_armed_for() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("test", 2);

        // Every step here is a jump, so each one spends a branch.
        observe(0x1000);
        observe(0x2000);
        assert!(is_armed());
        observe(0x3000);
        assert!(!is_armed());

        disarm();
    }

    #[test]
    fn a_trace_queued_while_one_is_running_gives_way() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        arm("first", 5);
        arm_when_drained("second", 5);
        drained();

        // Still the first one's trace, and the second is not waiting behind it.
        assert_eq!(LABEL.lock().clone().unwrap(), "first");
        assert!(WHEN_DRAINED.lock().is_none());

        disarm();
    }

    #[test]
    fn a_trace_queued_for_a_drain_waits_for_one() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm_when_drained("test", 4);
        assert!(!is_armed());

        drained();
        assert!(is_armed());

        disarm();
    }

    #[test]
    fn an_unanswered_request_traces_what_the_title_does_with_the_stand_in() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        over_an_unanswered_request("a frame nobody knows");

        // Queued, not started: the title has none of the stand-in yet.
        assert!(!is_armed());
        drained();
        assert!(is_armed());

        disarm();
    }

    #[test]
    fn a_title_that_keeps_asking_stops_being_traced() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        // A title answered by nobody asks again, often in a loop. The first few
        // attempts say what the later ones would.
        for _ in 0..UNANSWERED_TRACE_LIMIT {
            over_an_unanswered_request("again");
            assert!(WHEN_DRAINED.lock().is_some());
            *WHEN_DRAINED.lock() = None;
        }

        over_an_unanswered_request("and again");
        assert!(WHEN_DRAINED.lock().is_none(), "the log would fill with traces of one loop");

        disarm();
    }

    #[test]
    fn a_watched_address_starts_a_trace_when_it_is_reached() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        watch(0x3a364, "test", 100);
        assert!(is_watching());
        assert!(!is_armed());

        // Somewhere else first: a watch is for one address, not for any.
        reached(0x1000);
        assert!(!is_armed());

        reached(0x3a364);
        assert!(is_armed());

        disarm();
    }

    #[test]
    fn two_addresses_can_be_watched_at_once() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        // One address alone cannot tell "never reached" from "reached and
        // declined earlier"; the pair is what answers that.
        watch(0x35d6c, "handler", 100);
        watch(0x35f6e, "open", 100);

        reached(0x35f6e);
        assert!(is_armed());
        assert_eq!(LABEL.lock().clone().unwrap(), "open #1");

        REMAINING.store(0, Ordering::Relaxed);
        reached(0x35d6c);
        assert_eq!(LABEL.lock().clone().unwrap(), "handler #1");

        disarm();
    }

    #[test]
    fn one_watched_address_giving_up_leaves_the_other_watching() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        watch(0x35d6c, "handler", 100);
        watch(0x35f6e, "open", 100);

        for _ in 0..WATCH_TRACE_LIMIT + 1 {
            REMAINING.store(0, Ordering::Relaxed);
            reached(0x35d6c);
        }

        assert!(is_watching(), "the second address should still be watched");

        REMAINING.store(0, Ordering::Relaxed);
        reached(0x35f6e);
        assert!(is_armed());

        disarm();
    }

    #[test]
    fn a_watched_address_stops_being_watched_once_it_has_spoken() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        // A draw routine runs every frame; the first few passes say what the
        // rest would.
        watch(0x3a364, "test", 100);
        for _ in 0..WATCH_TRACE_LIMIT {
            REMAINING.store(0, Ordering::Relaxed);
            reached(0x3a364);
            assert!(is_armed());
        }

        REMAINING.store(0, Ordering::Relaxed);
        reached(0x3a364);
        assert!(!is_armed());
        assert!(!is_watching(), "the per-instruction check should cost nothing again");

        disarm();
    }

    #[test]
    fn a_specification_arms_what_it_names() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        set_watches("pc:3a376/500, w:0x1518700").unwrap();

        assert!(is_watching());
        assert!(is_write_watching());
        assert_eq!(watches(), "pc:pc:3a376/500,w:1518700");

        // Reaching the address starts a trace of the length it asked for.
        reached(0x3a376);
        assert!(is_armed());

        disarm();
    }

    #[test]
    fn an_empty_specification_clears_everything() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        set_watches("pc:3a376,w:1518700").unwrap();
        set_watches("").unwrap();

        assert!(!is_watching());
        assert!(!is_write_watching());

        disarm();
    }

    #[test]
    fn a_specification_that_makes_no_sense_says_so_and_arms_nothing() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        for bad in ["3a376", "pc 3a376", "x:1000", "pc:zzz", "pc:1000/many"] {
            assert!(set_watches(bad).is_err(), "{bad:?} was accepted");
            assert!(!is_watching(), "{bad:?} armed something");
            assert!(!is_write_watching(), "{bad:?} armed something");
        }

        disarm();
    }

    #[test]
    fn a_watched_write_is_reported_whatever_width_touches_it() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        watch_write(0x1518702, "test");

        // A byte store into the middle of the watched word still counts, which
        // is the case a naive equality check would miss.
        at(0x1234);
        wrote(0x1518700, 4, 7);

        let reports = WRITE_WATCHES.lock()[0].reports;
        assert_eq!(reports, 1);

        // A write that does not reach it does not.
        wrote(0x1518710, 4, 7);
        assert_eq!(WRITE_WATCHES.lock()[0].reports, 1);

        disarm();
    }

    #[test]
    fn a_drain_with_nothing_queued_arms_nothing() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();

        drained();

        assert!(!is_armed());
    }

    #[test]
    fn a_trace_line_names_what_armed_it() {
        let _guard = ONE_AT_A_TIME.lock();
        disarm();
        arm("서든어택", 8);

        assert_eq!(
            trace_line(&[Edge {
                from: 0x1a28,
                to: 0x520b4,
                times: 1
            }]),
            "probe 서든어택: 1a28>520b4"
        );
        disarm();
    }
}

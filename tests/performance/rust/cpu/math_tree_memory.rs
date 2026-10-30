//! 报告 Caret 数学树导入的分配量、保留堆大小和进程峰值 RSS。

use caret_doc::{MathNode, MathTree, NodeId};
use caret_engine::{ImportedMath, import_parsed_math};
use caret_math::{MathInputKind, parse};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every operation delegates the original pointer and layout to `System`; counters only
// observe successfully allocated sizes and do not alter allocation ownership or alignment.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies a valid allocation layout per `GlobalAlloc`'s contract.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies a valid allocation layout per `GlobalAlloc`'s contract.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout are forwarded unchanged to the allocator that created it.
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller supplies the original pointer/layout pair and a valid new size.
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
            adjust_live_bytes(layout.size(), new_size);
        }
        new_pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn record_allocation(size: usize) {
    ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED_BYTES.fetch_add(size, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
}

fn adjust_live_bytes(old_size: usize, new_size: usize) {
    let live = if new_size >= old_size {
        LIVE_BYTES.fetch_add(new_size - old_size, Ordering::Relaxed) + (new_size - old_size)
    } else {
        LIVE_BYTES.fetch_sub(old_size - new_size, Ordering::Relaxed) - (old_size - new_size)
    };
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
}

fn tree_for(imported: &ImportedMath) -> &MathTree {
    match imported {
        ImportedMath::Inline(value) => &value.math_tree,
        ImportedMath::Equation(value) => &value.math_tree,
        ImportedMath::Align(_) => panic!("the memory probe only builds inline formulas"),
    }
}

fn tree_metrics(tree: &MathTree) -> (usize, usize) {
    let mut count = 0usize;
    let mut maximum_depth = 0usize;
    let mut pending: Vec<(&MathNode, usize)> = vec![(&tree.root, 1)];
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        maximum_depth = maximum_depth.max(depth);
        pending.extend(node.children.iter().map(|child| (child, depth + 1)));
    }
    (count, maximum_depth)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn peak_rss_bytes() -> Option<usize> {
    // SAFETY: `usage` is a valid writable `rusage` value and `getrusage` only fills that struct.
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    // SAFETY: the pointer is valid for the duration of the call and uses the supported RUSAGE_SELF selector.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if status != 0 {
        return None;
    }
    let rss = usize::try_from(usage.ru_maxrss).ok()?;
    #[cfg(target_os = "linux")]
    return rss.checked_mul(1024);
    #[cfg(target_os = "macos")]
    Some(rss)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peak_rss_bytes() -> Option<usize> {
    None
}

fn main() {
    let sample = std::env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("usage: math_tree_memory <typical|deep|many>"));
    let (source, formula_count) = match sample.as_str() {
        "typical" => (
            r"\frac{\alpha_i + \beta_i}{\sqrt{x_i^2 + 1}} = \gamma_i".to_owned(),
            1,
        ),
        "deep" => (format!("{}x{}", r"\frac{1}{".repeat(10), "}".repeat(10)), 1),
        "many" => (
            r"\frac{\alpha_i + \beta_i}{\sqrt{x_i^2 + 1}} = \gamma_i".to_owned(),
            1_000,
        ),
        other => panic!("unknown math memory sample: {other}"),
    };
    let parsed = parse(&source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("{sample} formula did not parse: {error}"));
    drop(
        import_parsed_math(&parsed)
            .unwrap_or_else(|error| panic!("{sample} warmup import failed: {error}")),
    );
    let baseline_live = LIVE_BYTES.load(Ordering::Relaxed);
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    PEAK_LIVE_BYTES.store(baseline_live, Ordering::Relaxed);
    let mut imported = Vec::with_capacity(formula_count);

    for _ in 0..formula_count {
        imported.push(
            import_parsed_math(&parsed)
                .unwrap_or_else(|error| panic!("{sample} formula did not import: {error}")),
        );
    }

    let allocation_calls = ALLOCATION_CALLS.load(Ordering::Relaxed);
    let allocated_bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
    let retained_heap_bytes = LIVE_BYTES.load(Ordering::Relaxed) - baseline_live;
    let peak_heap_bytes = PEAK_LIVE_BYTES.load(Ordering::Relaxed) - baseline_live;
    let source_bytes = source.len() * formula_count;
    let (node_count, maximum_depth) = imported
        .iter()
        .map(|item| tree_metrics(tree_for(item)))
        .fold((0, 0), |(nodes, depth), metrics| {
            (nodes + metrics.0, depth.max(metrics.1))
        });
    let peak_rss_bytes = peak_rss_bytes();
    black_box(&imported);

    println!(
        "sample={sample} formulas={formula_count} source_bytes={source_bytes} nodes={node_count} max_depth={maximum_depth} math_node_size_bytes={} node_id_size_bytes={} import_allocation_calls={allocation_calls} import_allocated_bytes={allocated_bytes} retained_heap_bytes={retained_heap_bytes} peak_heap_bytes={peak_heap_bytes} process_peak_rss_bytes={peak_rss_bytes:?}",
        std::mem::size_of::<MathNode>(),
        std::mem::size_of::<NodeId>()
    );
}

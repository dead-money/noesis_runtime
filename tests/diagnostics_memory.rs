//! TODO §17 — Noesis allocator counters.
//!
//! `allocated_memory` / `allocated_memory_accum` / `allocations_count` are
//! process-global, so the test asserts **deltas** and **monotonicity** rather
//! than absolute values: hold a batch of freshly-parsed `FrameworkElement`s and
//! the live-allocation count must rise; the cumulative `accum` counter must
//! never decrease across reads.

use dm_noesis_runtime::diagnostics as diag;
use dm_noesis_runtime::view::FrameworkElement;

// A small but non-trivial element tree — each parse allocates many Noesis
// objects (the Grid, the Button, their DPs / visual children).
const XAML: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="B" Content="Hello" Width="80" Height="24"/>
</Grid>"##;

#[test]
fn allocator_counters_track_real_objects() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // Warm up the parser once so first-use lazy allocations (caches, type
        // metadata) are not attributed to the measured batch.
        drop(FrameworkElement::parse(XAML).expect("warmup parse failed"));

        let accum0 = diag::allocated_memory_accum();
        let count0 = diag::allocations_count();
        let bytes0 = diag::allocated_memory();

        // Allocate and HOLD a batch of real element trees.
        let mut kept: Vec<FrameworkElement> = Vec::new();
        for _ in 0..32 {
            kept.push(FrameworkElement::parse(XAML).expect("parse failed"));
        }

        let accum1 = diag::allocated_memory_accum();
        let count1 = diag::allocations_count();
        let bytes1 = diag::allocated_memory();

        // accum is cumulative → never decreases, and allocating 32 trees must
        // have pushed it up.
        assert!(
            accum1 >= accum0,
            "GetAllocatedMemoryAccum must be monotonic non-decreasing ({accum0} -> {accum1})"
        );
        assert!(
            accum1 > accum0,
            "allocating 32 element trees must increase the cumulative accum ({accum0} -> {accum1})"
        );

        // Live allocation count must be strictly higher while the batch is held.
        assert!(
            count1 > count0,
            "live allocations_count must rise while 32 trees are held ({count0} -> {count1})"
        );

        // Live byte usage must be strictly higher while the batch is held.
        assert!(
            bytes1 > bytes0,
            "live allocated_memory must rise while 32 trees are held ({bytes0} -> {bytes1})"
        );

        // Free the batch. NOTE: the live `allocations_count` does NOT reliably
        // drop right away in a headless process — Noesis services part of its
        // teardown (deferred deletes) from the render/update pump, which never
        // runs here, and unrelated internal allocations happen between reads. So
        // we do NOT assert the live count fell. What we CAN assert is that the
        // cumulative `accum` counter stayed monotonic across the free.
        let accum_peak = accum1;
        drop(kept);

        let accum2 = diag::allocated_memory_accum();
        assert!(
            accum2 >= accum_peak,
            "cumulative accum must not decrease when objects are freed ({accum_peak} -> {accum2})"
        );
    }

    dm_noesis_runtime::shutdown();
}

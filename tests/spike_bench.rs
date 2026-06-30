//! Spike-Bench — for a 200-row bound object list, where does the cost of a
//! single-row mutation live: in the FFI marshalling we do from Rust (the
//! "Apply" phase), or in Noesis-side container re-realization during the
//! `view.update` "Drive" phase?
//!
//! Three strategies mutate ONE row of an otherwise-stable 200-row list:
//!   (a) Reset:    `clear()` + re-push all 200 (the current rebuild path).
//!   (b) DP set:   one in-place `ClassInstance` DP write (no collection change).
//!   (c) Granular: one `remove_at` + one `insert_component` (Remove + Add).
//!
//! The list uses a NON-virtualizing `StackPanel` items panel so all 200
//! containers are realized — this is the pessimistic case that exposes
//! re-realization cost. For each strategy we time the Apply phase and the Drive
//! phase separately and read Noesis's cumulative allocation counter
//! (`allocated_memory_accum`) across Drive, so container churn shows up as
//! allocated bytes.
//!
//! This is a measurement spike, not a pass/fail gate: it asserts only that the
//! list stayed consistent, and prints the numbers the design decision needs.

use std::time::Instant;

use noesis_runtime::binding::ObservableCollection;
use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::diagnostics as diag;
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};

const ROWS: usize = 200;
const REPS: usize = 20;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ItemsControl xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
              xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
              x:Name="List" Width="200" Height="4000">
  <ItemsControl.ItemsPanel>
    <ItemsPanelTemplate>
      <StackPanel/>
    </ItemsPanelTemplate>
  </ItemsControl.ItemsPanel>
  <ItemsControl.ItemTemplate>
    <DataTemplate>
      <TextBlock Text="{Binding Name}" Height="18"/>
    </DataTemplate>
  </ItemsControl.ItemTemplate>
</ItemsControl>"##;

struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

struct Phase {
    apply_us: f64,
    drive_us: f64,
    drive_accum_delta: i64,
}

#[test]
fn spike_bench_single_row_mutation_costs() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = ClassBuilder::new("DmSpike.BenchRow", ClassBase::Freezable, Noop);
        let name_prop = builder.add_property("Name", PropType::String);
        let reg = builder.register().expect("register BenchRow");

        // 200 row objects held for the whole run.
        let mut rows = Vec::with_capacity(ROWS);
        for i in 0..ROWS {
            let inst = reg.create_instance().expect("create_instance");
            inst.handle().set_string(name_prop, &format!("Row {i}"));
            rows.push(inst);
        }

        let mut coll = ObservableCollection::new();
        for inst in &rows {
            coll.push_object(inst);
        }

        let root = FrameworkElement::parse(XAML).expect("parse XAML");
        let mut view = View::create(root);
        view.set_size(200, 4000);
        view.activate();

        let mut clock = 0.0_f64;
        let mut pump = |view: &mut View, n: usize| {
            for _ in 0..n {
                clock += 0.016;
                view.update(clock);
            }
        };
        pump(&mut view, 4);

        let mut content = view.content().expect("view content");
        let mut list = content.find_name("List").expect("find List");
        assert!(list.set_items_source(&coll));
        pump(&mut view, 6);
        assert_eq!(list.items_count(), Some(ROWS));
        assert_eq!(
            list.realized_item_count(),
            Some(ROWS),
            "non-virtualizing panel should realize all {ROWS} containers"
        );

        // ---- (a) Reset: clear + re-push all 200 -----------------------------
        let mut reset = Phase {
            apply_us: 0.0,
            drive_us: 0.0,
            drive_accum_delta: 0,
        };
        for _ in 0..REPS {
            let t = Instant::now();
            coll.clear();
            for inst in &rows {
                coll.push_object(inst);
            }
            reset.apply_us += t.elapsed().as_micros() as f64;

            let a0 = diag::allocated_memory_accum();
            let t = Instant::now();
            pump(&mut view, 2);
            reset.drive_us += t.elapsed().as_micros() as f64;
            reset.drive_accum_delta += i64::from(diag::allocated_memory_accum()) - i64::from(a0);
        }
        assert_eq!(list.realized_item_count(), Some(ROWS));

        // ---- (b) DP set: one in-place ClassInstance write -------------------
        let mut dp = Phase {
            apply_us: 0.0,
            drive_us: 0.0,
            drive_accum_delta: 0,
        };
        for r in 0..REPS {
            let k = r % ROWS;
            let t = Instant::now();
            rows[k]
                .handle()
                .set_string(name_prop, &format!("Edited {r}"));
            dp.apply_us += t.elapsed().as_micros() as f64;

            let a0 = diag::allocated_memory_accum();
            let t = Instant::now();
            pump(&mut view, 2);
            dp.drive_us += t.elapsed().as_micros() as f64;
            dp.drive_accum_delta += i64::from(diag::allocated_memory_accum()) - i64::from(a0);
        }
        assert_eq!(list.realized_item_count(), Some(ROWS));

        // ---- (c) Granular: one remove + one insert (Remove + Add) -----------
        let mut gran = Phase {
            apply_us: 0.0,
            drive_us: 0.0,
            drive_accum_delta: 0,
        };
        for r in 0..REPS {
            let k = (r * 7) % ROWS;
            let inst = &rows[k];
            let t = Instant::now();
            coll.remove_at(k);
            // SAFETY: `inst` is a live row object held in `rows` for the run.
            unsafe {
                coll.insert_component(k, inst.raw());
            }
            gran.apply_us += t.elapsed().as_micros() as f64;

            let a0 = diag::allocated_memory_accum();
            let t = Instant::now();
            pump(&mut view, 2);
            gran.drive_us += t.elapsed().as_micros() as f64;
            gran.drive_accum_delta += i64::from(diag::allocated_memory_accum()) - i64::from(a0);
        }
        assert_eq!(coll.len(), ROWS, "granular churn kept the row count stable");

        let report = |label: &str, p: &Phase| {
            eprintln!(
                "SPIKE-BENCH {label:8}: apply {:8.1} us/rep | drive {:9.1} us/rep | \
                 drive accum +{:>10} B/rep",
                p.apply_us / REPS as f64,
                p.drive_us / REPS as f64,
                p.drive_accum_delta / REPS as i64,
            );
        };
        eprintln!("SPIKE-BENCH: {ROWS} rows, {REPS} reps, non-virtualizing (all realized)");
        report("RESET", &reset);
        report("DP_SET", &dp);
        report("GRANULAR", &gran);

        content.clear_items_source();
        drop(list);
        drop(content);
        view.deactivate();
        drop(view);
        coll.clear();
        drop(coll);
        drop(rows);
        drop(reg);
    }

    noesis_runtime::shutdown();
}

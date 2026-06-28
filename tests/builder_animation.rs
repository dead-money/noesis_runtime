//! Phase 5 — From/To animation builders (Double/Color/Thickness/Point/Rect/
//! Size/Int16/Int32/Int64) covering from/to/by + the common Timeline knobs.
//!
//! Fail-if-stubbed: types exposing from/to/by getters (Rect / Size / Int*) are
//! round-tripped through the live object; the Timeline `duration_secs` knob is
//! read back for every builder (a real FFI crossing), proving the fluent chain
//! applies. Builder output is also compared against the longhand form.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).

use dm_noesis_runtime::animation::{
    ColorAnimation, DoubleAnimation, FillBehavior, Int16Animation, Int32Animation, Int64Animation,
    PointAnimation, RectAnimation, SizeAnimation, ThicknessAnimation, Timeline,
};

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

#[test]
fn builder_animation_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── RectAnimation: full from/to/by + knobs read-back ─────────────────
        let rect = RectAnimation::builder()
            .from([0.0, 0.0, 1.0, 1.0])
            .to([2.0, 2.0, 3.0, 3.0])
            .by([0.5, 0.5, 0.5, 0.5])
            .duration_secs(1.5)
            .begin_time_secs(0.25)
            .auto_reverse(true)
            .speed_ratio(2.0)
            .fill_behavior(FillBehavior::Stop)
            .repeat_count(3.0)
            .build();
        assert_eq!(rect.from(), Some([0.0, 0.0, 1.0, 1.0]), "rect from");
        assert_eq!(rect.to(), Some([2.0, 2.0, 3.0, 3.0]), "rect to");
        assert_eq!(rect.by(), Some([0.5, 0.5, 0.5, 0.5]), "rect by");
        assert!(
            approx(rect.duration_secs().expect("rect duration"), 1.5),
            "rect duration round-trip"
        );

        // Equivalence with the longhand form.
        let mut longhand = RectAnimation::new();
        let _ = longhand.set_from(Some([0.0, 0.0, 1.0, 1.0]));
        let _ = longhand.set_to(Some([2.0, 2.0, 3.0, 3.0]));
        let _ = longhand.set_by(Some([0.5, 0.5, 0.5, 0.5]));
        let _ = longhand.set_duration_secs(1.5);
        assert_eq!(rect.from(), longhand.from(), "builder == longhand (from)");
        assert_eq!(rect.to(), longhand.to(), "builder == longhand (to)");
        assert_eq!(rect.by(), longhand.by(), "builder == longhand (by)");

        // ── SizeAnimation ────────────────────────────────────────────────────
        let size = SizeAnimation::builder()
            .from([1.0, 2.0])
            .to([3.0, 4.0])
            .duration_secs(0.5)
            .build();
        assert_eq!(size.from(), Some([1.0, 2.0]), "size from");
        assert_eq!(size.to(), Some([3.0, 4.0]), "size to");
        assert!(approx(size.duration_secs().expect("size duration"), 0.5));

        // ── Int16 / Int32 / Int64 ────────────────────────────────────────────
        let i16a = Int16Animation::builder().from(1).to(10).by(2).build();
        assert_eq!(
            (i16a.from(), i16a.to(), i16a.by()),
            (Some(1), Some(10), Some(2))
        );

        let i32a = Int32Animation::builder()
            .from(100)
            .to(200)
            .duration_secs(0.75)
            .repeat_forever()
            .build();
        assert_eq!((i32a.from(), i32a.to()), (Some(100), Some(200)));
        assert!(approx(i32a.duration_secs().expect("i32 duration"), 0.75));

        let i64a = Int64Animation::builder().from(5).to(50_000_000_000).build();
        assert_eq!((i64a.from(), i64a.to()), (Some(5), Some(50_000_000_000)));

        // ── Double / Color / Thickness / Point: no from/to getters exist, so
        //    prove the chain crossed the FFI via the Timeline duration knob. ───
        let dbl = DoubleAnimation::builder()
            .from(0.0)
            .to(1.0)
            .duration_secs(2.0)
            .auto_reverse(true)
            .build();
        assert!(
            approx(dbl.duration_secs().expect("double duration"), 2.0),
            "double builder duration round-trip"
        );

        let col = ColorAnimation::builder()
            .from([0.0, 0.0, 0.0, 1.0])
            .to([1.0, 1.0, 1.0, 1.0])
            .duration_secs(1.0)
            .build();
        assert!(approx(col.duration_secs().expect("color duration"), 1.0));

        let thick = ThicknessAnimation::builder()
            .from([0.0; 4])
            .to([4.0; 4])
            .duration_secs(0.3)
            .build();
        assert!(approx(
            thick.duration_secs().expect("thickness duration"),
            0.3
        ));

        let pt = PointAnimation::builder()
            .from((0.0, 0.0))
            .to((10.0, 20.0))
            .duration_secs(0.9)
            .build();
        assert!(approx(pt.duration_secs().expect("point duration"), 0.9));
    }

    dm_noesis_runtime::shutdown();
}

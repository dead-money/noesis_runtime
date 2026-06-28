//! Code-built animation & timing (TODO §6 / Phase C): construct `Storyboard`s,
//! the common animation classes (`DoubleAnimation` / `ColorAnimation` /
//! `ThicknessAnimation` / `PointAnimation`), their key-frame variants, and the
//! easing-function family from Rust, then run them off the [`View`] clock.
//!
//! Each handle here owns a freshly-created Noesis object holding a single `+1`
//! reference, released on [`Drop`] via `dm_noesis_base_component_release` — the
//! same idiom as [`crate::brushes`]. Adding an animation to a [`Storyboard`], or
//! a key frame / easing function to its parent, makes Noesis take its own
//! reference, so the Rust builder handle may be dropped after wiring.
//!
//! # Running an animation
//!
//! Animations advance off the view's `TimeManager`, which is pumped by
//! [`View::update`](crate::view::View::update). Build the element tree, create a
//! `View`, then either:
//!
//! - **Storyboard route:** set [`Animation::set_target_name`] +
//!   [`Animation::set_target_property`] on each child, [`Storyboard::add_child`]
//!   them, and [`Storyboard::begin`] against the connected root element; or
//! - **Direct route:** [`Animation::begin_on`] a single animation onto a named
//!   element's dependency property (a `BeginAnimation` / `ApplyAnimationClock`
//!   equivalent off the element's view `TimeManager`).
//!
//! Then pump `view.update(t)` for increasing `t` across the duration and read
//! the animated value back through Noesis (e.g.
//! [`FrameworkElement::get_f32`](crate::view::FrameworkElement::get_f32)).

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    dm_noesis_animation_begin_on, dm_noesis_animation_set_easing_function,
    dm_noesis_base_component_release, dm_noesis_color_animation_add_keyframe,
    dm_noesis_color_animation_create, dm_noesis_color_animation_keyframes_create,
    dm_noesis_color_animation_set_by, dm_noesis_color_animation_set_from,
    dm_noesis_color_animation_set_to, dm_noesis_double_animation_add_keyframe,
    dm_noesis_double_animation_create, dm_noesis_double_animation_keyframes_create,
    dm_noesis_double_animation_set_by, dm_noesis_double_animation_set_from,
    dm_noesis_double_animation_set_to, dm_noesis_easing_function_create,
    dm_noesis_easing_function_set_amplitude, dm_noesis_easing_function_set_exponent,
    dm_noesis_easing_function_set_oscillations, dm_noesis_easing_function_set_power,
    dm_noesis_easing_function_set_springiness, dm_noesis_point_animation_create,
    dm_noesis_point_animation_set_by, dm_noesis_point_animation_set_from,
    dm_noesis_point_animation_set_to, dm_noesis_storyboard_add_child, dm_noesis_storyboard_begin,
    dm_noesis_storyboard_child_count, dm_noesis_storyboard_create, dm_noesis_storyboard_is_paused,
    dm_noesis_storyboard_is_playing, dm_noesis_storyboard_pause, dm_noesis_storyboard_resume,
    dm_noesis_storyboard_seek, dm_noesis_storyboard_set_target_name,
    dm_noesis_storyboard_set_target_property, dm_noesis_storyboard_stop,
    dm_noesis_thickness_animation_create, dm_noesis_thickness_animation_set_by,
    dm_noesis_thickness_animation_set_from, dm_noesis_thickness_animation_set_to,
    dm_noesis_timeline_get_duration_seconds, dm_noesis_timeline_set_auto_reverse,
    dm_noesis_timeline_set_begin_time_seconds, dm_noesis_timeline_set_duration_auto,
    dm_noesis_timeline_set_duration_forever, dm_noesis_timeline_set_duration_seconds,
    dm_noesis_timeline_set_fill_behavior, dm_noesis_timeline_set_repeat_count,
    dm_noesis_timeline_set_repeat_duration, dm_noesis_timeline_set_repeat_forever,
    dm_noesis_timeline_set_speed_ratio,
};
use crate::view::FrameworkElement;

/// How an easing function interpolates over the animation's progress. Ordinals
/// match `Noesis::EasingMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EasingMode {
    /// `100% - f(t)` — decelerating.
    EaseOut = 0,
    /// `f(t)` — accelerating.
    EaseIn = 1,
    /// `EaseIn` for the first half, `EaseOut` for the second.
    EaseInOut = 2,
}

/// The concrete easing curve. Ordinals match the `kind` switch in
/// `cpp/noesis_animation.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EasingKind {
    /// `t^2`.
    Quadratic = 0,
    /// `t^3`.
    Cubic = 1,
    /// `t^4`.
    Quartic = 2,
    /// `t^5`.
    Quintic = 3,
    /// Sinusoidal.
    Sine = 4,
    /// Circular arc.
    Circle = 5,
    /// Retracts slightly before moving (see [`EasingFunction::set_amplitude`]).
    Back = 6,
    /// Bouncing (see [`EasingFunction::set_oscillations`] /
    /// [`EasingFunction::set_springiness`]).
    Bounce = 7,
    /// Spring-like oscillation.
    Elastic = 8,
    /// Exponential (see [`EasingFunction::set_exponent`]).
    Exponential = 9,
    /// Configurable power (see [`EasingFunction::set_power`]).
    Power = 10,
}

/// How a timeline behaves once it reaches the end of its active period.
/// Ordinals match `Noesis::FillBehavior`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FillBehavior {
    /// Hold the final animated value after completion.
    HoldEnd = 0,
    /// Release the animated value (revert to base) after completion.
    Stop = 1,
}

/// How a newly-started animation interacts with one already running on the same
/// property. Ordinals match `Noesis::HandoffBehavior`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandoffBehavior {
    /// Snapshot the current value and replace any running animation.
    SnapshotAndReplace = 0,
    /// Compose with any running animation.
    Compose = 1,
}

/// The interpolation method of a single key frame. Ordinals match the `kind`
/// switch in `cpp/noesis_animation.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyFrameKind {
    /// Jump to the value at the key time (no interpolation).
    Discrete = 0,
    /// Linear interpolation up to the key time.
    Linear = 1,
    /// Eased interpolation (provide an [`EasingFunction`]).
    Easing = 2,
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: a Noesis BaseComponent handle; same single-threaded-per-object
        // affinity as the other owning wrappers in this crate.
        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref that
                // we own; released exactly once here.
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// Common `Timeline` knobs shared by every animation type (duration, repeat,
/// auto-reverse, …). Implemented through the object's raw `Timeline*`.
pub trait Timeline {
    /// Borrowed `Noesis::Timeline*` for `self`.
    fn timeline_raw(&self) -> *mut c_void;

    /// Set the single-pass duration in seconds.
    fn set_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_duration_seconds(self.timeline_raw(), seconds) }
    }

    /// Set `Duration="Automatic"` (resolved from the content, e.g. key frames).
    fn set_duration_auto(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_duration_auto(self.timeline_raw()) }
    }

    /// Set `Duration="Forever"`.
    fn set_duration_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_duration_forever(self.timeline_raw()) }
    }

    /// Read the configured single-pass duration in seconds, or `None` if the
    /// duration is `Automatic` / `Forever` (not a resolved `TimeSpan`).
    fn duration_secs(&self) -> Option<f64> {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        let s = unsafe { dm_noesis_timeline_get_duration_seconds(self.timeline_raw()) };
        (s >= 0.0).then_some(s)
    }

    /// Delay before the timeline begins, in seconds.
    fn set_begin_time_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_begin_time_seconds(self.timeline_raw(), seconds) }
    }

    /// Play forwards then backwards each iteration when `true`.
    fn set_auto_reverse(&mut self, value: bool) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_auto_reverse(self.timeline_raw(), value) }
    }

    /// Rate at which time progresses relative to the parent (default `1.0`).
    fn set_speed_ratio(&mut self, value: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_speed_ratio(self.timeline_raw(), value) }
    }

    /// Behaviour once the active period ends (hold the end value or release it).
    fn set_fill_behavior(&mut self, behavior: FillBehavior) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_fill_behavior(self.timeline_raw(), behavior as i32) }
    }

    /// Repeat a fixed number of (possibly fractional) iterations.
    fn set_repeat_count(&mut self, count: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_repeat_count(self.timeline_raw(), count) }
    }

    /// Repeat for a fixed wall-clock duration, in seconds.
    fn set_repeat_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_repeat_duration(self.timeline_raw(), seconds) }
    }

    /// Repeat forever.
    fn set_repeat_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { dm_noesis_timeline_set_repeat_forever(self.timeline_raw()) }
    }
}

/// An animation timeline that can target a property and be run via a
/// [`Storyboard`] or directly with [`begin_on`](Animation::begin_on).
pub trait Animation: Timeline {
    /// Borrowed `Noesis::AnimationTimeline*` for `self`.
    fn animation_raw(&self) -> *mut c_void;

    /// Set this animation's `Storyboard.TargetName` — the `x:Name` of the
    /// element it drives, resolved against the namescope passed to
    /// [`Storyboard::begin`].
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    fn set_target_name(&mut self, name: &str) -> bool {
        let c = CString::new(name).expect("target name contained interior NUL");
        // SAFETY: animation_raw() is a live DependencyObject*; c lives for the call.
        unsafe { dm_noesis_storyboard_set_target_name(self.animation_raw(), c.as_ptr()) }
    }

    /// Set this animation's `Storyboard.TargetProperty` — the property path it
    /// drives (e.g. `"Opacity"`,
    /// `"(UIElement.RenderTransform).(ScaleTransform.ScaleX)"`).
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte.
    fn set_target_property(&mut self, path: &str) -> bool {
        let c = CString::new(path).expect("target property contained interior NUL");
        // SAFETY: animation_raw() is a live DependencyObject*; c lives for the call.
        unsafe { dm_noesis_storyboard_set_target_property(self.animation_raw(), c.as_ptr()) }
    }

    /// Attach an easing function. No-op (returns `false`) for key-frame
    /// animations, whose easing is configured per key frame instead.
    fn set_easing(&mut self, easing: &EasingFunction) -> bool {
        // SAFETY: both pointers are live for the call; Noesis takes its own ref
        // to the easing function.
        unsafe { dm_noesis_animation_set_easing_function(self.animation_raw(), easing.raw()) }
    }

    /// Start this animation directly on `target`'s `dp_name` dependency
    /// property, using the target's view `TimeManager` (a `BeginAnimation` /
    /// `ApplyAnimationClock` equivalent). `target` must be connected to a live
    /// [`View`](crate::view::View) (so it has a `TimeManager`). Returns `false`
    /// on an unknown property or a disconnected target.
    ///
    /// # Panics
    ///
    /// Panics if `dp_name` contains an interior NUL byte.
    fn begin_on(
        &mut self,
        target: &FrameworkElement,
        dp_name: &str,
        handoff: HandoffBehavior,
    ) -> bool {
        let c = CString::new(dp_name).expect("dp name contained interior NUL");
        // SAFETY: animation_raw() and target.raw() are live for the call; c lives
        // for the call; the C side resolves the DP and the TimeManager.
        unsafe {
            dm_noesis_animation_begin_on(
                self.animation_raw(),
                target.raw(),
                c.as_ptr(),
                handoff as i32,
            )
        }
    }
}

// ── Storyboard ───────────────────────────────────────────────────────────────

/// A `Storyboard` — a container timeline that targets and runs its child
/// animations.
pub struct Storyboard {
    ptr: NonNull<c_void>,
}

base_component_handle!(Storyboard);

impl Timeline for Storyboard {
    fn timeline_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl Default for Storyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Storyboard {
    /// Create an empty `Storyboard`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Storyboard*.
        let ptr = unsafe { dm_noesis_storyboard_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_storyboard_create returned null"),
        }
    }

    /// Add a child animation. The storyboard's collection takes its own
    /// reference, so `anim` may be dropped afterwards. Returns `false` on a type
    /// mismatch.
    pub fn add_child<A: Animation>(&mut self, anim: &A) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_add_child(self.raw(), anim.animation_raw()) }
    }

    /// Number of child animations, or `None` if the handle is not a Storyboard
    /// (should not happen for a live handle).
    #[must_use]
    pub fn child_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is a live Storyboard*.
        let n = unsafe { dm_noesis_storyboard_child_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Apply this storyboard's animations to their targets and start them.
    /// `root` is both the target tree root and the namescope used to resolve
    /// each child's [`Animation::set_target_name`]; it must be connected to a
    /// live [`View`](crate::view::View). Pass `controllable = true` to enable
    /// [`pause`](Self::pause) / [`resume`](Self::resume) / [`stop`](Self::stop)
    /// / [`seek`](Self::seek).
    pub fn begin(&mut self, root: &FrameworkElement, controllable: bool) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_begin(self.raw(), root.raw(), controllable) }
    }

    /// Pause the controllable clocks created for `root`. No-op unless the
    /// storyboard was [`begin`](Self::begin)-run with `controllable = true`.
    pub fn pause(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_pause(self.raw(), root.raw()) }
    }

    /// Resume the controllable clocks created for `root`.
    pub fn resume(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_resume(self.raw(), root.raw()) }
    }

    /// Stop the controllable clocks created for `root`.
    pub fn stop(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_stop(self.raw(), root.raw()) }
    }

    /// Seek the controllable clocks created for `root` to `seconds` from the
    /// beginning, applied on the next clock tick.
    pub fn seek(&mut self, root: &FrameworkElement, seconds: f64) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_seek(self.raw(), root.raw(), seconds) }
    }

    /// Whether a controllable storyboard is currently playing for `root`.
    #[must_use]
    pub fn is_playing(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_is_playing(self.raw(), root.raw()) }
    }

    /// Whether a controllable storyboard is currently paused for `root`.
    #[must_use]
    pub fn is_paused(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { dm_noesis_storyboard_is_paused(self.raw(), root.raw()) }
    }
}

// ── Easing functions ─────────────────────────────────────────────────────────

/// An easing function applied to a From/To animation or an easing key frame.
pub struct EasingFunction {
    ptr: NonNull<c_void>,
}

base_component_handle!(EasingFunction);

impl EasingFunction {
    /// Create an easing function of `kind` with interpolation `mode`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new(kind: EasingKind, mode: EasingMode) -> Self {
        // SAFETY: factory returns a +1-owned EasingFunctionBase*.
        let ptr = unsafe { dm_noesis_easing_function_create(kind as i32, mode as i32) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_easing_function_create returned null"),
        }
    }

    /// Set `BackEase.Amplitude` (the retraction amount). No-op (returns `false`)
    /// on other easing kinds.
    pub fn set_amplitude(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { dm_noesis_easing_function_set_amplitude(self.raw(), value) }
    }

    /// Set `PowerEase.Power`. No-op on other kinds.
    pub fn set_power(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { dm_noesis_easing_function_set_power(self.raw(), value) }
    }

    /// Set `ExponentialEase.Exponent`. No-op on other kinds.
    pub fn set_exponent(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { dm_noesis_easing_function_set_exponent(self.raw(), value) }
    }

    /// Set `ElasticEase.Oscillations` / `BounceEase.Bounces`. No-op on other
    /// kinds.
    pub fn set_oscillations(&mut self, value: i32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { dm_noesis_easing_function_set_oscillations(self.raw(), value) }
    }

    /// Set `ElasticEase.Springiness` / `BounceEase.Bounciness`. No-op on other
    /// kinds.
    pub fn set_springiness(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { dm_noesis_easing_function_set_springiness(self.raw(), value) }
    }
}

// ── From/To/By animations ────────────────────────────────────────────────────

macro_rules! animation_impls {
    ($name:ident) => {
        base_component_handle!($name);

        impl Timeline for $name {
            fn timeline_raw(&self) -> *mut c_void {
                self.raw()
            }
        }

        impl Animation for $name {
            fn animation_raw(&self) -> *mut c_void {
                self.raw()
            }
        }
    };
}

/// A `DoubleAnimation` — interpolates a `float` property linearly between
/// `From` and `To` over the duration.
pub struct DoubleAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(DoubleAnimation);

impl Default for DoubleAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl DoubleAnimation {
    /// Create an empty `DoubleAnimation` (set `From`/`To`/`Duration` next).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimation*.
        let ptr = unsafe { dm_noesis_double_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_double_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting value.
    pub fn set_from(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe {
            dm_noesis_double_animation_set_from(self.raw(), value.is_some(), value.unwrap_or(0.0))
        }
    }

    /// Set (`Some`) or clear (`None`) the ending value.
    pub fn set_to(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe {
            dm_noesis_double_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0.0))
        }
    }

    /// Set (`Some`) or clear (`None`) the relative offset (`By`).
    pub fn set_by(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe {
            dm_noesis_double_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0.0))
        }
    }
}

/// A `ColorAnimation` — interpolates a `Color` property linearly between `From`
/// and `To`. Colors are `[r, g, b, a]`, each `0..=1`.
pub struct ColorAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(ColorAnimation);

impl Default for ColorAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorAnimation {
    /// Create an empty `ColorAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimation*.
        let ptr = unsafe { dm_noesis_color_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_color_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting color.
    pub fn set_from(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { dm_noesis_color_animation_set_from(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending color.
    pub fn set_to(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { dm_noesis_color_animation_set_to(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative color offset (`By`).
    pub fn set_by(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { dm_noesis_color_animation_set_by(self.raw(), rgba.is_some(), v.as_ptr()) }
    }
}

/// A `ThicknessAnimation` — interpolates a `Thickness` property
/// (`[left, top, right, bottom]`).
pub struct ThicknessAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(ThicknessAnimation);

impl Default for ThicknessAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl ThicknessAnimation {
    /// Create an empty `ThicknessAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ThicknessAnimation*.
        let ptr = unsafe { dm_noesis_thickness_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_thickness_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting thickness.
    pub fn set_from(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { dm_noesis_thickness_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending thickness.
    pub fn set_to(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { dm_noesis_thickness_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative thickness offset (`By`).
    pub fn set_by(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { dm_noesis_thickness_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }
}

/// A `PointAnimation` — interpolates a `Point` property (`(x, y)`).
pub struct PointAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(PointAnimation);

impl Default for PointAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl PointAnimation {
    /// Create an empty `PointAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned PointAnimation*.
        let ptr = unsafe { dm_noesis_point_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_point_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting point.
    pub fn set_from(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { dm_noesis_point_animation_set_from(self.raw(), value.is_some(), x, y) }
    }

    /// Set (`Some`) or clear (`None`) the ending point.
    pub fn set_to(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { dm_noesis_point_animation_set_to(self.raw(), value.is_some(), x, y) }
    }

    /// Set (`Some`) or clear (`None`) the relative point offset (`By`).
    pub fn set_by(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { dm_noesis_point_animation_set_by(self.raw(), value.is_some(), x, y) }
    }
}

// ── Key-frame animations ─────────────────────────────────────────────────────

/// A `DoubleAnimationUsingKeyFrames` — animates a `float` property through a
/// sequence of discrete / linear / eased key frames.
pub struct DoubleAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(DoubleAnimationUsingKeyFrames);

impl Default for DoubleAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl DoubleAnimationUsingKeyFrames {
    /// Create an empty key-frame double animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimationUsingKeyFrames*.
        let ptr = unsafe { dm_noesis_double_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("dm_noesis_double_animation_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` at `key_time_secs`. For
    /// [`KeyFrameKind::Easing`], pass the `easing` function (ignored otherwise).
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: f32,
        easing: Option<&EasingFunction>,
    ) -> bool {
        let e = easing.map_or(core::ptr::null_mut(), EasingFunction::raw);
        // SAFETY: self.raw() is a live keyframe animation; `e` is null or a live
        // easing function; both are only read during the call.
        unsafe {
            dm_noesis_double_animation_add_keyframe(
                self.raw(),
                kind as i32,
                key_time_secs,
                value,
                e,
            )
        }
    }
}

/// A `ColorAnimationUsingKeyFrames` — animates a `Color` property through a
/// sequence of color key frames.
pub struct ColorAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(ColorAnimationUsingKeyFrames);

impl Default for ColorAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorAnimationUsingKeyFrames {
    /// Create an empty key-frame color animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimationUsingKeyFrames*.
        let ptr = unsafe { dm_noesis_color_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("dm_noesis_color_animation_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `rgba` at `key_time_secs`. For
    /// [`KeyFrameKind::Easing`], pass the `easing` function (ignored otherwise).
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        rgba: [f32; 4],
        easing: Option<&EasingFunction>,
    ) -> bool {
        let e = easing.map_or(core::ptr::null_mut(), EasingFunction::raw);
        // SAFETY: self.raw() is a live keyframe animation; `rgba` outlives the
        // call; `e` is null or a live easing function.
        unsafe {
            dm_noesis_color_animation_add_keyframe(
                self.raw(),
                kind as i32,
                key_time_secs,
                rgba.as_ptr(),
                e,
            )
        }
    }
}

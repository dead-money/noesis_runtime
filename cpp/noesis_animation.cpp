// Code-built animation & timing (TODO §6 / Phase C): Storyboard, the common
// animation classes (Double/Color/Thickness/Point), their key-frame variants
// (Discrete/Linear/Easing for Double and Color), the easing-function family,
// and a storyboard-less BeginAnimation path off the view's TimeManager.
//
// Ownership mirrors cpp/noesis_binding.cpp / cpp/noesis_brushes.cpp: every
// `*_create` hands out exactly one owned reference (the owning Rust handle in
// src/animation.rs releases it on Drop via dm_noesis_base_component_release).
// Adding a timeline to a Storyboard's TimelineCollection, or assigning a key
// frame / easing function to its parent, makes Noesis take its own reference,
// so the Rust builder handle can be dropped after wiring.
//
// Animations advance off the View clock: drive them with view.update(t) for
// increasing t. A target element must be connected to a live View before
// Storyboard::Begin / AnimationTimeline::Start can resolve a TimeManager.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Nullable.h>
#include <NsCore/Ptr.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/Symbol.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Thickness.h>
#include <NsGui/AnimationTimeline.h>
#include <NsGui/BackEase.h>
#include <NsGui/BaseKeyFrame.h>
#include <NsGui/BounceEase.h>
#include <NsGui/CircleEase.h>
#include <NsGui/ColorAnimation.h>
#include <NsGui/ColorAnimationUsingKeyFrames.h>
#include <NsGui/ColorKeyFrame.h>
#include <NsGui/CubicEase.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/DiscreteColorKeyFrame.h>
#include <NsGui/DiscreteDoubleKeyFrame.h>
#include <NsGui/DoubleAnimation.h>
#include <NsGui/DoubleAnimationUsingKeyFrames.h>
#include <NsGui/DoubleKeyFrame.h>
#include <NsGui/Duration.h>
#include <NsGui/EasingColorKeyFrame.h>
#include <NsGui/EasingDoubleKeyFrame.h>
#include <NsGui/EasingFunctionBase.h>
#include <NsGui/ElasticEase.h>
#include <NsGui/ExponentialEase.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/FreezableCollection.h>
#include <NsGui/HandoffBehavior.h>
#include <NsGui/IUITreeNode.h>
#include <NsGui/KeyTime.h>
#include <NsGui/LinearColorKeyFrame.h>
#include <NsGui/LinearDoubleKeyFrame.h>
#include <NsGui/PointAnimation.h>
#include <NsGui/PowerEase.h>
#include <NsGui/PropertyPath.h>
#include <NsGui/QuadraticEase.h>
#include <NsGui/QuarticEase.h>
#include <NsGui/QuinticEase.h>
#include <NsGui/RepeatBehavior.h>
#include <NsGui/SineEase.h>
#include <NsGui/Storyboard.h>
#include <NsGui/ThicknessAnimation.h>
#include <NsGui/TimeSpan.h>
#include <NsGui/Timeline.h>
#include <NsGui/TimelineGroup.h>

namespace {

// Hand a freshly-created (refcount-1) BaseComponent out across the C ABI with
// exactly one reference owned by the caller; the local Ptr releases its own on
// scope exit, leaving the caller's +1. Same idiom as cpp/noesis_brushes.cpp.
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

template <class T>
T* cast(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<T*>(static_cast<Noesis::BaseComponent*>(p));
}

// EasingMode ordinals must match Noesis::EasingMode (EaseOut/EaseIn/EaseInOut).
static_assert(Noesis::EasingMode_EaseOut == 0, "EasingMode ordinal drift");
static_assert(Noesis::EasingMode_EaseIn == 1, "EasingMode ordinal drift");
static_assert(Noesis::EasingMode_EaseInOut == 2, "EasingMode ordinal drift");

// FillBehavior ordinals must match Noesis::FillBehavior (HoldEnd/Stop).
static_assert(Noesis::FillBehavior_HoldEnd == 0, "FillBehavior ordinal drift");
static_assert(Noesis::FillBehavior_Stop == 1, "FillBehavior ordinal drift");

}  // namespace

// ── Storyboard ───────────────────────────────────────────────────────────────

extern "C" void* dm_noesis_storyboard_create() {
    Noesis::Ptr<Noesis::Storyboard> sb = *new Noesis::Storyboard();
    return handout(sb.GetPtr());
}

// Append a child Timeline (an animation) to the Storyboard's children
// collection, creating it if absent. The collection takes its own reference;
// the caller keeps ownership of `timeline`. Returns false on type mismatch.
extern "C" bool dm_noesis_storyboard_add_child(void* sb, void* timeline) {
    auto* s = cast<Noesis::Storyboard>(sb);
    auto* t = cast<Noesis::Timeline>(timeline);
    if (!s || !t) return false;
    Noesis::TimelineCollection* children = s->GetChildren();
    if (!children) {
        Noesis::Ptr<Noesis::TimelineCollection> created = *new Noesis::TimelineCollection();
        s->SetChildren(created.GetPtr());
        children = created.GetPtr();
    }
    children->Add(t);
    return true;
}

extern "C" int32_t dm_noesis_storyboard_child_count(void* sb) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return -1;
    Noesis::TimelineCollection* children = s->GetChildren();
    return children ? children->Count() : 0;
}

// Storyboard.TargetName attached property — names the element a child animation
// targets, resolved against the namescope passed to Begin. `timeline` is the
// child animation (a DependencyObject).
extern "C" bool dm_noesis_storyboard_set_target_name(void* timeline, const char* name) {
    auto* d = cast<Noesis::DependencyObject>(timeline);
    if (!d || !name) return false;
    Noesis::Storyboard::SetTargetName(d, name);
    return true;
}

// Storyboard.TargetProperty attached property — the property path the child
// animation drives (e.g. "Opacity", "(UIElement.RenderTransform).(ScaleX)").
extern "C" bool dm_noesis_storyboard_set_target_property(void* timeline, const char* path) {
    auto* d = cast<Noesis::DependencyObject>(timeline);
    if (!d || !path) return false;
    Noesis::Ptr<Noesis::PropertyPath> pp = *new Noesis::PropertyPath(path);
    Noesis::Storyboard::SetTargetProperty(d, pp.GetPtr());
    return true;
}

// Storyboard.Target attached property — a direct object reference, an
// alternative to TargetName when the target isn't in a namescope.
extern "C" bool dm_noesis_storyboard_set_target(void* timeline, void* target) {
    auto* d = cast<Noesis::DependencyObject>(timeline);
    auto* tg = cast<Noesis::DependencyObject>(target);
    if (!d) return false;
    Noesis::Storyboard::SetTarget(d, tg);
    return true;
}

// Begin the storyboard. `fe` (nullable) is both the target root and namescope
// used to resolve TargetName. `controllable` must be true for the
// Pause/Resume/Stop/Seek actions to have any effect.
extern "C" bool dm_noesis_storyboard_begin(void* sb, void* fe, bool controllable) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (f) {
        s->Begin(f, controllable);
    } else {
        s->Begin();
    }
    return true;
}

extern "C" bool dm_noesis_storyboard_pause(void* sb, void* fe) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (f) {
        s->Pause(f);
    } else {
        s->Pause();
    }
    return true;
}

extern "C" bool dm_noesis_storyboard_resume(void* sb, void* fe) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (f) {
        s->Resume(f);
    } else {
        s->Resume();
    }
    return true;
}

extern "C" bool dm_noesis_storyboard_stop(void* sb, void* fe) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (f) {
        s->Stop(f);
    } else {
        s->Stop();
    }
    return true;
}

extern "C" bool dm_noesis_storyboard_seek(void* sb, void* fe, double seconds) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    Noesis::TimeSpan offset(seconds);
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (f) {
        s->Seek(f, offset, Noesis::TimeSeekOrigin_BeginTime);
    } else {
        s->Seek(offset);
    }
    return true;
}

extern "C" bool dm_noesis_storyboard_is_playing(void* sb, void* fe) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    return f ? s->IsPlaying(f) : s->IsPlaying();
}

extern "C" bool dm_noesis_storyboard_is_paused(void* sb, void* fe) {
    auto* s = cast<Noesis::Storyboard>(sb);
    if (!s) return false;
    auto* f = cast<Noesis::FrameworkElement>(fe);
    return f ? s->IsPaused(f) : s->IsPaused();
}

// ── Timeline common knobs (apply to any Timeline / animation) ────────────────

extern "C" bool dm_noesis_timeline_set_duration_seconds(void* tl, double seconds) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetDuration(Noesis::Duration(Noesis::TimeSpan(seconds)));
    return true;
}

extern "C" bool dm_noesis_timeline_set_duration_auto(void* tl) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetDuration(Noesis::Duration::Automatic());
    return true;
}

extern "C" bool dm_noesis_timeline_set_duration_forever(void* tl) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetDuration(Noesis::Duration::Forever());
    return true;
}

// Returns the duration in seconds, or -1.0 if the duration is not a resolved
// TimeSpan (Automatic / Forever / not a Timeline).
extern "C" double dm_noesis_timeline_get_duration_seconds(void* tl) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return -1.0;
    const Noesis::Duration& d = t->GetDuration();
    if (d.GetDurationType() != Noesis::DurationType_TimeSpan) return -1.0;
    return d.GetTimeSpan().GetTotalSeconds();
}

extern "C" bool dm_noesis_timeline_set_begin_time_seconds(void* tl, double seconds) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetBeginTime(Noesis::TimeSpan(seconds));
    return true;
}

extern "C" bool dm_noesis_timeline_set_auto_reverse(void* tl, bool value) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetAutoReverse(value);
    return true;
}

extern "C" bool dm_noesis_timeline_set_speed_ratio(void* tl, float value) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetSpeedRatio(value);
    return true;
}

// behavior: 0 = HoldEnd, 1 = Stop (matches Noesis::FillBehavior).
extern "C" bool dm_noesis_timeline_set_fill_behavior(void* tl, int32_t behavior) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetFillBehavior(static_cast<Noesis::FillBehavior>(behavior));
    return true;
}

extern "C" bool dm_noesis_timeline_set_repeat_count(void* tl, float count) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetRepeatBehavior(Noesis::RepeatBehavior(count));
    return true;
}

extern "C" bool dm_noesis_timeline_set_repeat_duration(void* tl, double seconds) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetRepeatBehavior(Noesis::RepeatBehavior(Noesis::TimeSpan(seconds)));
    return true;
}

extern "C" bool dm_noesis_timeline_set_repeat_forever(void* tl) {
    auto* t = cast<Noesis::Timeline>(tl);
    if (!t) return false;
    t->SetRepeatBehavior(Noesis::RepeatBehavior::Forever());
    return true;
}

// ── From/To/By animations ────────────────────────────────────────────────────

extern "C" void* dm_noesis_double_animation_create() {
    Noesis::Ptr<Noesis::DoubleAnimation> a = *new Noesis::DoubleAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_double_animation_set_from(void* anim, bool has, float v) {
    auto* a = cast<Noesis::DoubleAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has ? Noesis::Nullable<float>(v) : Noesis::Nullable<float>());
    return true;
}

extern "C" bool dm_noesis_double_animation_set_to(void* anim, bool has, float v) {
    auto* a = cast<Noesis::DoubleAnimation>(anim);
    if (!a) return false;
    a->SetTo(has ? Noesis::Nullable<float>(v) : Noesis::Nullable<float>());
    return true;
}

extern "C" bool dm_noesis_double_animation_set_by(void* anim, bool has, float v) {
    auto* a = cast<Noesis::DoubleAnimation>(anim);
    if (!a) return false;
    a->SetBy(has ? Noesis::Nullable<float>(v) : Noesis::Nullable<float>());
    return true;
}

extern "C" void* dm_noesis_color_animation_create() {
    Noesis::Ptr<Noesis::ColorAnimation> a = *new Noesis::ColorAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_color_animation_set_from(void* anim, bool has, const float color[4]) {
    auto* a = cast<Noesis::ColorAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has && color ? Noesis::Nullable<Noesis::Color>(
                                  Noesis::Color(color[0], color[1], color[2], color[3]))
                            : Noesis::Nullable<Noesis::Color>());
    return true;
}

extern "C" bool dm_noesis_color_animation_set_to(void* anim, bool has, const float color[4]) {
    auto* a = cast<Noesis::ColorAnimation>(anim);
    if (!a) return false;
    a->SetTo(has && color ? Noesis::Nullable<Noesis::Color>(
                                Noesis::Color(color[0], color[1], color[2], color[3]))
                          : Noesis::Nullable<Noesis::Color>());
    return true;
}

extern "C" bool dm_noesis_color_animation_set_by(void* anim, bool has, const float color[4]) {
    auto* a = cast<Noesis::ColorAnimation>(anim);
    if (!a) return false;
    a->SetBy(has && color ? Noesis::Nullable<Noesis::Color>(
                                Noesis::Color(color[0], color[1], color[2], color[3]))
                          : Noesis::Nullable<Noesis::Color>());
    return true;
}

extern "C" void* dm_noesis_thickness_animation_create() {
    Noesis::Ptr<Noesis::ThicknessAnimation> a = *new Noesis::ThicknessAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_thickness_animation_set_from(void* anim, bool has, const float t[4]) {
    auto* a = cast<Noesis::ThicknessAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has && t ? Noesis::Nullable<Noesis::Thickness>(
                              Noesis::Thickness(t[0], t[1], t[2], t[3]))
                        : Noesis::Nullable<Noesis::Thickness>());
    return true;
}

extern "C" bool dm_noesis_thickness_animation_set_to(void* anim, bool has, const float t[4]) {
    auto* a = cast<Noesis::ThicknessAnimation>(anim);
    if (!a) return false;
    a->SetTo(has && t ? Noesis::Nullable<Noesis::Thickness>(
                            Noesis::Thickness(t[0], t[1], t[2], t[3]))
                      : Noesis::Nullable<Noesis::Thickness>());
    return true;
}

extern "C" bool dm_noesis_thickness_animation_set_by(void* anim, bool has, const float t[4]) {
    auto* a = cast<Noesis::ThicknessAnimation>(anim);
    if (!a) return false;
    a->SetBy(has && t ? Noesis::Nullable<Noesis::Thickness>(
                            Noesis::Thickness(t[0], t[1], t[2], t[3]))
                      : Noesis::Nullable<Noesis::Thickness>());
    return true;
}

extern "C" void* dm_noesis_point_animation_create() {
    Noesis::Ptr<Noesis::PointAnimation> a = *new Noesis::PointAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_point_animation_set_from(void* anim, bool has, float x, float y) {
    auto* a = cast<Noesis::PointAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has ? Noesis::Nullable<Noesis::Point>(Noesis::Point(x, y))
                   : Noesis::Nullable<Noesis::Point>());
    return true;
}

extern "C" bool dm_noesis_point_animation_set_to(void* anim, bool has, float x, float y) {
    auto* a = cast<Noesis::PointAnimation>(anim);
    if (!a) return false;
    a->SetTo(has ? Noesis::Nullable<Noesis::Point>(Noesis::Point(x, y))
                 : Noesis::Nullable<Noesis::Point>());
    return true;
}

extern "C" bool dm_noesis_point_animation_set_by(void* anim, bool has, float x, float y) {
    auto* a = cast<Noesis::PointAnimation>(anim);
    if (!a) return false;
    a->SetBy(has ? Noesis::Nullable<Noesis::Point>(Noesis::Point(x, y))
                 : Noesis::Nullable<Noesis::Point>());
    return true;
}

// Attach an easing function to a From/To animation. DynamicCasts across the
// supported animation types. Noesis takes its own reference to `easing`.
extern "C" bool dm_noesis_animation_set_easing_function(void* anim, void* easing) {
    auto* e = cast<Noesis::EasingFunctionBase>(easing);  // may be null to clear
    if (auto* d = cast<Noesis::DoubleAnimation>(anim)) {
        d->SetEasingFunction(e);
        return true;
    }
    if (auto* c = cast<Noesis::ColorAnimation>(anim)) {
        c->SetEasingFunction(e);
        return true;
    }
    if (auto* t = cast<Noesis::ThicknessAnimation>(anim)) {
        t->SetEasingFunction(e);
        return true;
    }
    if (auto* p = cast<Noesis::PointAnimation>(anim)) {
        p->SetEasingFunction(e);
        return true;
    }
    return false;
}

// ── Easing functions ─────────────────────────────────────────────────────────
//
// kind: 0 Quadratic, 1 Cubic, 2 Quartic, 3 Quintic, 4 Sine, 5 Circle, 6 Back,
//       7 Bounce, 8 Elastic, 9 Exponential, 10 Power.
// mode: matches Noesis::EasingMode (0 EaseOut, 1 EaseIn, 2 EaseInOut).
extern "C" void* dm_noesis_easing_function_create(int32_t kind, int32_t mode) {
    Noesis::Ptr<Noesis::EasingFunctionBase> e;
    switch (kind) {
        case 0: e = *new Noesis::QuadraticEase(); break;
        case 1: e = *new Noesis::CubicEase(); break;
        case 2: e = *new Noesis::QuarticEase(); break;
        case 3: e = *new Noesis::QuinticEase(); break;
        case 4: e = *new Noesis::SineEase(); break;
        case 5: e = *new Noesis::CircleEase(); break;
        case 6: e = *new Noesis::BackEase(); break;
        case 7: e = *new Noesis::BounceEase(); break;
        case 8: e = *new Noesis::ElasticEase(); break;
        case 9: e = *new Noesis::ExponentialEase(); break;
        case 10: e = *new Noesis::PowerEase(); break;
        default: return nullptr;
    }
    e->SetEasingMode(static_cast<Noesis::EasingMode>(mode));
    return handout(e.GetPtr());
}

// BackEase.Amplitude.
extern "C" bool dm_noesis_easing_function_set_amplitude(void* easing, float value) {
    auto* b = cast<Noesis::BackEase>(easing);
    if (!b) return false;
    b->SetAmplitude(value);
    return true;
}

// PowerEase.Power.
extern "C" bool dm_noesis_easing_function_set_power(void* easing, float value) {
    auto* p = cast<Noesis::PowerEase>(easing);
    if (!p) return false;
    p->SetPower(value);
    return true;
}

// ExponentialEase.Exponent.
extern "C" bool dm_noesis_easing_function_set_exponent(void* easing, float value) {
    auto* e = cast<Noesis::ExponentialEase>(easing);
    if (!e) return false;
    e->SetExponent(value);
    return true;
}

// ElasticEase.Oscillations / BounceEase.Bounces (both integer counts).
extern "C" bool dm_noesis_easing_function_set_oscillations(void* easing, int32_t value) {
    if (auto* el = cast<Noesis::ElasticEase>(easing)) {
        el->SetOscillations(value);
        return true;
    }
    if (auto* bo = cast<Noesis::BounceEase>(easing)) {
        bo->SetBounces(value);
        return true;
    }
    return false;
}

// ElasticEase.Springiness / BounceEase.Bounciness.
extern "C" bool dm_noesis_easing_function_set_springiness(void* easing, float value) {
    if (auto* el = cast<Noesis::ElasticEase>(easing)) {
        el->SetSpringiness(value);
        return true;
    }
    if (auto* bo = cast<Noesis::BounceEase>(easing)) {
        bo->SetBounciness(value);
        return true;
    }
    return false;
}

// ── Key-frame animations ─────────────────────────────────────────────────────

extern "C" void* dm_noesis_double_animation_keyframes_create() {
    Noesis::Ptr<Noesis::DoubleAnimationUsingKeyFrames> a =
        *new Noesis::DoubleAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

// kind: 0 Discrete, 1 Linear, 2 Easing (uses `easing` if non-null).
extern "C" bool dm_noesis_double_animation_add_keyframe(void* anim, int32_t kind,
                                                        double key_time_seconds, float value,
                                                        void* easing) {
    auto* a = cast<Noesis::DoubleAnimationUsingKeyFrames>(anim);
    if (!a) return false;
    Noesis::DoubleKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;

    Noesis::Ptr<Noesis::DoubleKeyFrame> kf;
    switch (kind) {
        case 0: kf = *new Noesis::DiscreteDoubleKeyFrame(); break;
        case 1: kf = *new Noesis::LinearDoubleKeyFrame(); break;
        case 2: {
            Noesis::Ptr<Noesis::EasingDoubleKeyFrame> ekf = *new Noesis::EasingDoubleKeyFrame();
            if (auto* e = cast<Noesis::EasingFunctionBase>(easing)) ekf->SetEasingFunction(e);
            kf = ekf;
            break;
        }
        default: return false;
    }
    kf->SetValue(value);
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" void* dm_noesis_color_animation_keyframes_create() {
    Noesis::Ptr<Noesis::ColorAnimationUsingKeyFrames> a =
        *new Noesis::ColorAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_color_animation_add_keyframe(void* anim, int32_t kind,
                                                       double key_time_seconds,
                                                       const float color[4], void* easing) {
    auto* a = cast<Noesis::ColorAnimationUsingKeyFrames>(anim);
    if (!a || !color) return false;
    Noesis::ColorKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;

    Noesis::Ptr<Noesis::ColorKeyFrame> kf;
    switch (kind) {
        case 0: kf = *new Noesis::DiscreteColorKeyFrame(); break;
        case 1: kf = *new Noesis::LinearColorKeyFrame(); break;
        case 2: {
            Noesis::Ptr<Noesis::EasingColorKeyFrame> ekf = *new Noesis::EasingColorKeyFrame();
            if (auto* e = cast<Noesis::EasingFunctionBase>(easing)) ekf->SetEasingFunction(e);
            kf = ekf;
            break;
        }
        default: return false;
    }
    kf->SetValue(Noesis::Color(color[0], color[1], color[2], color[3]));
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

// ── Storyboard-less direct animation (BeginAnimation / ApplyAnimationClock) ──
//
// Resolve `dp_name` against `target`'s type, fetch the TimeManager from the
// connected FrameworkElement, and Start the animation directly on that property.
// `target` MUST be a FrameworkElement attached to a live View (so it has a
// TimeManager). handoff: matches Noesis::HandoffBehavior (0 SnapshotAndReplace,
// 1 Compose). Returns false on null/type mismatch, unknown property, or no
// TimeManager (target not connected to a view).
extern "C" bool dm_noesis_animation_begin_on(void* anim, void* target, const char* dp_name,
                                             int32_t handoff) {
    auto* a = cast<Noesis::AnimationTimeline>(anim);
    auto* fe = cast<Noesis::FrameworkElement>(target);
    if (!a || !fe || !dp_name) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(fe->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return false;

    Noesis::ITimeManager* tm = fe->GetTimeManager();
    if (!tm) return false;

    a->Start(fe, dp, tm, static_cast<Noesis::HandoffBehavior>(handoff));
    return true;
}

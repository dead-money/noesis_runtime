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
#include <NsDrawing/Rect.h>
#include <NsDrawing/Size.h>
#include <NsDrawing/Thickness.h>
#include <NsGui/AnimationTimeline.h>
#include <NsGui/BackEase.h>
#include <NsGui/BaseKeyFrame.h>
#include <NsGui/BeginStoryboard.h>
#include <NsGui/BooleanAnimationUsingKeyFrames.h>
#include <NsGui/BooleanKeyFrame.h>
#include <NsGui/BounceEase.h>
#include <NsGui/CircleEase.h>
#include <NsGui/ColorAnimation.h>
#include <NsGui/ColorAnimationUsingKeyFrames.h>
#include <NsGui/ColorKeyFrame.h>
#include <NsGui/CubicEase.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/DiscreteBooleanKeyFrame.h>
#include <NsGui/DiscreteColorKeyFrame.h>
#include <NsGui/DiscreteDoubleKeyFrame.h>
#include <NsGui/DiscreteInt16KeyFrame.h>
#include <NsGui/DiscreteInt32KeyFrame.h>
#include <NsGui/DiscreteInt64KeyFrame.h>
#include <NsGui/DiscreteMatrixKeyFrame.h>
#include <NsGui/DiscreteObjectKeyFrame.h>
#include <NsGui/DiscretePointKeyFrame.h>
#include <NsGui/DiscreteRectKeyFrame.h>
#include <NsGui/DiscreteSizeKeyFrame.h>
#include <NsGui/DiscreteStringKeyFrame.h>
#include <NsGui/DiscreteThicknessKeyFrame.h>
#include <NsGui/DoubleAnimation.h>
#include <NsGui/DoubleAnimationUsingKeyFrames.h>
#include <NsGui/DoubleKeyFrame.h>
#include <NsGui/Duration.h>
#include <NsGui/EasingColorKeyFrame.h>
#include <NsGui/EasingDoubleKeyFrame.h>
#include <NsGui/EasingFunctionBase.h>
#include <NsGui/EasingInt16KeyFrame.h>
#include <NsGui/EasingInt32KeyFrame.h>
#include <NsGui/EasingInt64KeyFrame.h>
#include <NsGui/EasingPointKeyFrame.h>
#include <NsGui/EasingRectKeyFrame.h>
#include <NsGui/EasingSizeKeyFrame.h>
#include <NsGui/EasingThicknessKeyFrame.h>
#include <NsGui/ElasticEase.h>
#include <NsGui/ExponentialEase.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/FreezableCollection.h>
#include <NsGui/HandoffBehavior.h>
#include <NsGui/IUITreeNode.h>
#include <NsGui/Int16Animation.h>
#include <NsGui/Int16AnimationUsingKeyFrames.h>
#include <NsGui/Int16KeyFrame.h>
#include <NsGui/Int32Animation.h>
#include <NsGui/Int32AnimationUsingKeyFrames.h>
#include <NsGui/Int32KeyFrame.h>
#include <NsGui/Int64Animation.h>
#include <NsGui/Int64AnimationUsingKeyFrames.h>
#include <NsGui/Int64KeyFrame.h>
#include <NsGui/KeySpline.h>
#include <NsGui/KeyTime.h>
#include <NsGui/LinearColorKeyFrame.h>
#include <NsGui/LinearDoubleKeyFrame.h>
#include <NsGui/LinearInt16KeyFrame.h>
#include <NsGui/LinearInt32KeyFrame.h>
#include <NsGui/LinearInt64KeyFrame.h>
#include <NsGui/LinearPointKeyFrame.h>
#include <NsGui/LinearRectKeyFrame.h>
#include <NsGui/LinearSizeKeyFrame.h>
#include <NsGui/LinearThicknessKeyFrame.h>
#include <NsGui/MatrixAnimationUsingKeyFrames.h>
#include <NsGui/MatrixKeyFrame.h>
#include <NsGui/ObjectAnimationUsingKeyFrames.h>
#include <NsGui/ObjectKeyFrame.h>
#include <NsGui/ParallelTimeline.h>
#include <NsGui/PointAnimation.h>
#include <NsGui/PointAnimationUsingKeyFrames.h>
#include <NsGui/PointKeyFrame.h>
#include <NsGui/PowerEase.h>
#include <NsGui/PropertyPath.h>
#include <NsGui/QuadraticEase.h>
#include <NsGui/QuarticEase.h>
#include <NsGui/QuinticEase.h>
#include <NsGui/RectAnimation.h>
#include <NsGui/RectAnimationUsingKeyFrames.h>
#include <NsGui/RectKeyFrame.h>
#include <NsGui/RepeatBehavior.h>
#include <NsGui/SineEase.h>
#include <NsGui/SizeAnimation.h>
#include <NsGui/SizeAnimationUsingKeyFrames.h>
#include <NsGui/SizeKeyFrame.h>
#include <NsGui/SplineColorKeyFrame.h>
#include <NsGui/SplineDoubleKeyFrame.h>
#include <NsGui/SplineInt16KeyFrame.h>
#include <NsGui/SplineInt32KeyFrame.h>
#include <NsGui/SplineInt64KeyFrame.h>
#include <NsGui/SplinePointKeyFrame.h>
#include <NsGui/SplineRectKeyFrame.h>
#include <NsGui/SplineSizeKeyFrame.h>
#include <NsGui/SplineThicknessKeyFrame.h>
#include <NsGui/StringAnimationUsingKeyFrames.h>
#include <NsGui/StringKeyFrame.h>
#include <NsGui/Storyboard.h>
#include <NsGui/ThicknessAnimation.h>
#include <NsGui/ThicknessAnimationUsingKeyFrames.h>
#include <NsGui/ThicknessKeyFrame.h>
#include <NsGui/TimeSpan.h>
#include <NsGui/Timeline.h>
#include <NsGui/TimelineGroup.h>
#include <NsMath/Transform.h>

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

// HandoffBehavior ordinals must match Noesis::HandoffBehavior.
static_assert(Noesis::HandoffBehavior_SnapshotAndReplace == 0, "HandoffBehavior ordinal drift");
static_assert(Noesis::HandoffBehavior_Compose == 1, "HandoffBehavior ordinal drift");

// Build a Noesis::Rect from an {x, y, width, height} float[4] (the field layout
// the Rust side marshals), bypassing the side-coordinate Rect(l,t,r,b) ctor.
Noesis::Rect makeRect(const float r[4]) {
    Noesis::Rect rect;
    rect.x = r[0];
    rect.y = r[1];
    rect.width = r[2];
    rect.height = r[3];
    return rect;
}

void readRect(const Noesis::Rect& rect, float out[4]) {
    out[0] = rect.x;
    out[1] = rect.y;
    out[2] = rect.width;
    out[3] = rect.height;
}

// key-frame `kind`: 0 Discrete, 1 Linear, 2 Easing (uses `extra` as an
// EasingFunctionBase*), 3 Spline (uses `extra` as a KeySpline*). Returns a key
// frame with its KeyTime set; the caller sets the typed value. Disc/Lin/Eas/Spl
// are the concrete key-frame classes for the animated type.
template <class KF, class Disc, class Lin, class Eas, class Spl>
Noesis::Ptr<KF> makeKeyFrame(int32_t kind, double key_time_seconds, void* extra) {
    Noesis::Ptr<KF> kf;
    switch (kind) {
        case 0: kf = *new Disc(); break;
        case 1: kf = *new Lin(); break;
        case 2: {
            Noesis::Ptr<Eas> e = *new Eas();
            if (auto* ef = cast<Noesis::EasingFunctionBase>(extra)) e->SetEasingFunction(ef);
            kf = e;
            break;
        }
        case 3: {
            Noesis::Ptr<Spl> s = *new Spl();
            if (auto* ks = cast<Noesis::KeySpline>(extra)) s->SetKeySpline(ks);
            kf = s;
            break;
        }
        default: return nullptr;
    }
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    return kf;
}

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

// Begin the storyboard with an explicit HandoffBehavior (how its new clocks
// interact with animations already running on the same properties). `fe` is the
// target root + namescope and is required for this overload. handoff: matches
// Noesis::HandoffBehavior (0 SnapshotAndReplace, 1 Compose).
extern "C" bool dm_noesis_storyboard_begin_handoff(void* sb, void* fe, int32_t handoff,
                                                   bool controllable) {
    auto* s = cast<Noesis::Storyboard>(sb);
    auto* f = cast<Noesis::FrameworkElement>(fe);
    if (!s || !f) return false;
    s->Begin(f, static_cast<Noesis::HandoffBehavior>(handoff), controllable);
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
    if (auto* r = cast<Noesis::RectAnimation>(anim)) {
        r->SetEasingFunction(e);
        return true;
    }
    if (auto* s = cast<Noesis::SizeAnimation>(anim)) {
        s->SetEasingFunction(e);
        return true;
    }
    if (auto* i16 = cast<Noesis::Int16Animation>(anim)) {
        i16->SetEasingFunction(e);
        return true;
    }
    if (auto* i32 = cast<Noesis::Int32Animation>(anim)) {
        i32->SetEasingFunction(e);
        return true;
    }
    if (auto* i64 = cast<Noesis::Int64Animation>(anim)) {
        i64->SetEasingFunction(e);
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

// kind: 0 Discrete, 1 Linear, 2 Easing (`extra` = EasingFunctionBase*), 3 Spline
// (`extra` = KeySpline*).
extern "C" bool dm_noesis_double_animation_add_keyframe(void* anim, int32_t kind,
                                                        double key_time_seconds, float value,
                                                        void* extra) {
    auto* a = cast<Noesis::DoubleAnimationUsingKeyFrames>(anim);
    if (!a) return false;
    Noesis::DoubleKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::DoubleKeyFrame> kf =
        makeKeyFrame<Noesis::DoubleKeyFrame, Noesis::DiscreteDoubleKeyFrame,
                     Noesis::LinearDoubleKeyFrame, Noesis::EasingDoubleKeyFrame,
                     Noesis::SplineDoubleKeyFrame>(kind, key_time_seconds, extra);
    if (!kf) return false;
    kf->SetValue(value);
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" void* dm_noesis_color_animation_keyframes_create() {
    Noesis::Ptr<Noesis::ColorAnimationUsingKeyFrames> a =
        *new Noesis::ColorAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

// kind: 0 Discrete, 1 Linear, 2 Easing (`extra` = EasingFunctionBase*), 3 Spline
// (`extra` = KeySpline*).
extern "C" bool dm_noesis_color_animation_add_keyframe(void* anim, int32_t kind,
                                                       double key_time_seconds,
                                                       const float color[4], void* extra) {
    auto* a = cast<Noesis::ColorAnimationUsingKeyFrames>(anim);
    if (!a || !color) return false;
    Noesis::ColorKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::ColorKeyFrame> kf =
        makeKeyFrame<Noesis::ColorKeyFrame, Noesis::DiscreteColorKeyFrame,
                     Noesis::LinearColorKeyFrame, Noesis::EasingColorKeyFrame,
                     Noesis::SplineColorKeyFrame>(kind, key_time_seconds, extra);
    if (!kf) return false;
    kf->SetValue(Noesis::Color(color[0], color[1], color[2], color[3]));
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

// ── Rect / Size From-To animations ───────────────────────────────────────────
//
// Rect values cross the ABI as an {x, y, width, height} float[4]; Size values as
// a {width, height} float[2]. Each setter takes a `has` flag (false clears the
// Nullable); each getter fills `out` and returns whether the Nullable was set.

extern "C" void* dm_noesis_animation_rect_animation_create() {
    Noesis::Ptr<Noesis::RectAnimation> a = *new Noesis::RectAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_rect_animation_set_from(void* anim, bool has, const float r[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has && r ? Noesis::Nullable<Noesis::Rect>(makeRect(r)) : Noesis::Nullable<Noesis::Rect>());
    return true;
}

extern "C" bool dm_noesis_animation_rect_animation_set_to(void* anim, bool has, const float r[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a) return false;
    a->SetTo(has && r ? Noesis::Nullable<Noesis::Rect>(makeRect(r)) : Noesis::Nullable<Noesis::Rect>());
    return true;
}

extern "C" bool dm_noesis_animation_rect_animation_set_by(void* anim, bool has, const float r[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a) return false;
    a->SetBy(has && r ? Noesis::Nullable<Noesis::Rect>(makeRect(r)) : Noesis::Nullable<Noesis::Rect>());
    return true;
}

extern "C" bool dm_noesis_animation_rect_animation_get_from(void* anim, float out[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Rect>& n = a->GetFrom();
    if (!n.HasValue()) return false;
    readRect(n.GetValue(), out);
    return true;
}

extern "C" bool dm_noesis_animation_rect_animation_get_to(void* anim, float out[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Rect>& n = a->GetTo();
    if (!n.HasValue()) return false;
    readRect(n.GetValue(), out);
    return true;
}

extern "C" bool dm_noesis_animation_rect_animation_get_by(void* anim, float out[4]) {
    auto* a = cast<Noesis::RectAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Rect>& n = a->GetBy();
    if (!n.HasValue()) return false;
    readRect(n.GetValue(), out);
    return true;
}

extern "C" void* dm_noesis_animation_size_animation_create() {
    Noesis::Ptr<Noesis::SizeAnimation> a = *new Noesis::SizeAnimation();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_size_animation_set_from(void* anim, bool has, const float s[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a) return false;
    a->SetFrom(has && s ? Noesis::Nullable<Noesis::Size>(Noesis::Size(s[0], s[1]))
                        : Noesis::Nullable<Noesis::Size>());
    return true;
}

extern "C" bool dm_noesis_animation_size_animation_set_to(void* anim, bool has, const float s[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a) return false;
    a->SetTo(has && s ? Noesis::Nullable<Noesis::Size>(Noesis::Size(s[0], s[1]))
                      : Noesis::Nullable<Noesis::Size>());
    return true;
}

extern "C" bool dm_noesis_animation_size_animation_set_by(void* anim, bool has, const float s[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a) return false;
    a->SetBy(has && s ? Noesis::Nullable<Noesis::Size>(Noesis::Size(s[0], s[1]))
                      : Noesis::Nullable<Noesis::Size>());
    return true;
}

extern "C" bool dm_noesis_animation_size_animation_get_from(void* anim, float out[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Size>& n = a->GetFrom();
    if (!n.HasValue()) return false;
    out[0] = n.GetValue().width;
    out[1] = n.GetValue().height;
    return true;
}

extern "C" bool dm_noesis_animation_size_animation_get_to(void* anim, float out[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Size>& n = a->GetTo();
    if (!n.HasValue()) return false;
    out[0] = n.GetValue().width;
    out[1] = n.GetValue().height;
    return true;
}

extern "C" bool dm_noesis_animation_size_animation_get_by(void* anim, float out[2]) {
    auto* a = cast<Noesis::SizeAnimation>(anim);
    if (!a || !out) return false;
    const Noesis::Nullable<Noesis::Size>& n = a->GetBy();
    if (!n.HasValue()) return false;
    out[0] = n.GetValue().width;
    out[1] = n.GetValue().height;
    return true;
}

// ── Int16 / Int32 / Int64 From-To animations ─────────────────────────────────
//
// Int16/Int32 cross the ABI as int32_t (narrowed on the C++ side); Int64 as
// int64_t. Setters take a `has` flag; getters fill `*out` and return HasValue.

#define DM_INT_FROMTO(SUFFIX, CLASS, T, ABIT)                                                 \
    extern "C" void* dm_noesis_animation_##SUFFIX##_animation_create() {                      \
        Noesis::Ptr<Noesis::CLASS> a = *new Noesis::CLASS();                                  \
        return handout(a.GetPtr());                                                           \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_set_from(void* anim, bool has,   \
                                                                      ABIT v) {               \
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a) return false;                                                                 \
        a->SetFrom(has ? Noesis::Nullable<T>(static_cast<T>(v)) : Noesis::Nullable<T>());     \
        return true;                                                                          \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_set_to(void* anim, bool has,     \
                                                                    ABIT v) {                 \
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a) return false;                                                                 \
        a->SetTo(has ? Noesis::Nullable<T>(static_cast<T>(v)) : Noesis::Nullable<T>());       \
        return true;                                                                          \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_set_by(void* anim, bool has,     \
                                                                    ABIT v) {                 \
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a) return false;                                                                 \
        a->SetBy(has ? Noesis::Nullable<T>(static_cast<T>(v)) : Noesis::Nullable<T>());       \
        return true;                                                                          \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_get_from(void* anim, ABIT* out) {\
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a || !out) return false;                                                         \
        const Noesis::Nullable<T>& n = a->GetFrom();                                          \
        if (!n.HasValue()) return false;                                                      \
        *out = static_cast<ABIT>(n.GetValue());                                               \
        return true;                                                                          \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_get_to(void* anim, ABIT* out) {  \
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a || !out) return false;                                                         \
        const Noesis::Nullable<T>& n = a->GetTo();                                            \
        if (!n.HasValue()) return false;                                                      \
        *out = static_cast<ABIT>(n.GetValue());                                               \
        return true;                                                                          \
    }                                                                                         \
    extern "C" bool dm_noesis_animation_##SUFFIX##_animation_get_by(void* anim, ABIT* out) {  \
        auto* a = cast<Noesis::CLASS>(anim);                                                  \
        if (!a || !out) return false;                                                         \
        const Noesis::Nullable<T>& n = a->GetBy();                                            \
        if (!n.HasValue()) return false;                                                      \
        *out = static_cast<ABIT>(n.GetValue());                                               \
        return true;                                                                          \
    }

DM_INT_FROMTO(int16, Int16Animation, int16_t, int32_t)
DM_INT_FROMTO(int32, Int32Animation, int32_t, int32_t)
DM_INT_FROMTO(int64, Int64Animation, int64_t, int64_t)

#undef DM_INT_FROMTO

// ── Rect / Size key-frame animations ─────────────────────────────────────────
//
// `kind`: 0 Discrete, 1 Linear, 2 Easing (`extra` = EasingFunctionBase*), 3
// Spline (`extra` = KeySpline*). Read-back exposes the key-frame count, value,
// and key time so a test can prove each frame crossed.

extern "C" void* dm_noesis_animation_rect_keyframes_create() {
    Noesis::Ptr<Noesis::RectAnimationUsingKeyFrames> a = *new Noesis::RectAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_rect_keyframes_add(void* anim, int32_t kind,
                                                       double key_time_seconds, const float r[4],
                                                       void* extra) {
    auto* a = cast<Noesis::RectAnimationUsingKeyFrames>(anim);
    if (!a || !r) return false;
    Noesis::RectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::RectKeyFrame> kf =
        makeKeyFrame<Noesis::RectKeyFrame, Noesis::DiscreteRectKeyFrame, Noesis::LinearRectKeyFrame,
                     Noesis::EasingRectKeyFrame, Noesis::SplineRectKeyFrame>(kind, key_time_seconds,
                                                                            extra);
    if (!kf) return false;
    kf->SetValue(makeRect(r));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_rect_keyframes_count(void* anim) {
    auto* a = cast<Noesis::RectAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::RectKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" bool dm_noesis_animation_rect_keyframes_get_value(void* anim, int32_t index,
                                                             float out[4]) {
    auto* a = cast<Noesis::RectAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::RectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    readRect(frames->Get(static_cast<uint32_t>(index))->GetValue(), out);
    return true;
}

extern "C" double dm_noesis_animation_rect_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::RectAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::RectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

extern "C" void* dm_noesis_animation_size_keyframes_create() {
    Noesis::Ptr<Noesis::SizeAnimationUsingKeyFrames> a = *new Noesis::SizeAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_size_keyframes_add(void* anim, int32_t kind,
                                                       double key_time_seconds, const float s[2],
                                                       void* extra) {
    auto* a = cast<Noesis::SizeAnimationUsingKeyFrames>(anim);
    if (!a || !s) return false;
    Noesis::SizeKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::SizeKeyFrame> kf =
        makeKeyFrame<Noesis::SizeKeyFrame, Noesis::DiscreteSizeKeyFrame, Noesis::LinearSizeKeyFrame,
                     Noesis::EasingSizeKeyFrame, Noesis::SplineSizeKeyFrame>(kind, key_time_seconds,
                                                                            extra);
    if (!kf) return false;
    kf->SetValue(Noesis::Size(s[0], s[1]));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_size_keyframes_count(void* anim) {
    auto* a = cast<Noesis::SizeAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::SizeKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" bool dm_noesis_animation_size_keyframes_get_value(void* anim, int32_t index,
                                                             float out[2]) {
    auto* a = cast<Noesis::SizeAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::SizeKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    const Noesis::Size& v = frames->Get(static_cast<uint32_t>(index))->GetValue();
    out[0] = v.width;
    out[1] = v.height;
    return true;
}

extern "C" double dm_noesis_animation_size_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::SizeAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::SizeKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── Point key-frame animation ─────────────────────────────────────────────────
//
// `kind`: 0 Discrete, 1 Linear, 2 Easing (`extra` = EasingFunctionBase*), 3
// Spline (`extra` = KeySpline*). Points cross the ABI as {x, y} float[2].

extern "C" void* dm_noesis_animation_point_keyframes_create() {
    Noesis::Ptr<Noesis::PointAnimationUsingKeyFrames> a =
        *new Noesis::PointAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_point_keyframes_add(void* anim, int32_t kind,
                                                        double key_time_seconds, const float p[2],
                                                        void* extra) {
    auto* a = cast<Noesis::PointAnimationUsingKeyFrames>(anim);
    if (!a || !p) return false;
    Noesis::PointKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::PointKeyFrame> kf =
        makeKeyFrame<Noesis::PointKeyFrame, Noesis::DiscretePointKeyFrame,
                     Noesis::LinearPointKeyFrame, Noesis::EasingPointKeyFrame,
                     Noesis::SplinePointKeyFrame>(kind, key_time_seconds, extra);
    if (!kf) return false;
    kf->SetValue(Noesis::Point(p[0], p[1]));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_point_keyframes_count(void* anim) {
    auto* a = cast<Noesis::PointAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::PointKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" bool dm_noesis_animation_point_keyframes_get_value(void* anim, int32_t index,
                                                              float out[2]) {
    auto* a = cast<Noesis::PointAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::PointKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    const Noesis::Point& v = frames->Get(static_cast<uint32_t>(index))->GetValue();
    out[0] = v.x;
    out[1] = v.y;
    return true;
}

extern "C" double dm_noesis_animation_point_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::PointAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::PointKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── Thickness key-frame animation ─────────────────────────────────────────────
//
// `kind`: 0 Discrete, 1 Linear, 2 Easing (`extra` = EasingFunctionBase*), 3
// Spline (`extra` = KeySpline*). Thicknesses cross as {left, top, right, bottom}
// float[4].

extern "C" void* dm_noesis_animation_thickness_keyframes_create() {
    Noesis::Ptr<Noesis::ThicknessAnimationUsingKeyFrames> a =
        *new Noesis::ThicknessAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_thickness_keyframes_add(void* anim, int32_t kind,
                                                            double key_time_seconds,
                                                            const float t[4], void* extra) {
    auto* a = cast<Noesis::ThicknessAnimationUsingKeyFrames>(anim);
    if (!a || !t) return false;
    Noesis::ThicknessKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::ThicknessKeyFrame> kf =
        makeKeyFrame<Noesis::ThicknessKeyFrame, Noesis::DiscreteThicknessKeyFrame,
                     Noesis::LinearThicknessKeyFrame, Noesis::EasingThicknessKeyFrame,
                     Noesis::SplineThicknessKeyFrame>(kind, key_time_seconds, extra);
    if (!kf) return false;
    kf->SetValue(Noesis::Thickness(t[0], t[1], t[2], t[3]));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_thickness_keyframes_count(void* anim) {
    auto* a = cast<Noesis::ThicknessAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::ThicknessKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" bool dm_noesis_animation_thickness_keyframes_get_value(void* anim, int32_t index,
                                                                  float out[4]) {
    auto* a = cast<Noesis::ThicknessAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::ThicknessKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    const Noesis::Thickness& v = frames->Get(static_cast<uint32_t>(index))->GetValue();
    out[0] = v.left;
    out[1] = v.top;
    out[2] = v.right;
    out[3] = v.bottom;
    return true;
}

extern "C" double dm_noesis_animation_thickness_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::ThicknessAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::ThicknessKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── Int16 / Int32 / Int64 key-frame animations ───────────────────────────────

#define DM_INT_KEYFRAMES(SUFFIX, ANIM, COLL, KF, DISC, LIN, EAS, SPL, T, ABIT)                  \
    extern "C" void* dm_noesis_animation_##SUFFIX##_keyframes_create() {                        \
        Noesis::Ptr<Noesis::ANIM> a = *new Noesis::ANIM();                                      \
        return handout(a.GetPtr());                                                             \
    }                                                                                           \
    extern "C" bool dm_noesis_animation_##SUFFIX##_keyframes_add(                               \
        void* anim, int32_t kind, double key_time_seconds, ABIT value, void* extra) {           \
        auto* a = cast<Noesis::ANIM>(anim);                                                     \
        if (!a) return false;                                                                   \
        Noesis::COLL* frames = a->GetKeyFrames();                                               \
        if (!frames) return false;                                                              \
        Noesis::Ptr<Noesis::KF> kf =                                                            \
            makeKeyFrame<Noesis::KF, Noesis::DISC, Noesis::LIN, Noesis::EAS, Noesis::SPL>(      \
                kind, key_time_seconds, extra);                                                 \
        if (!kf) return false;                                                                  \
        kf->SetValue(static_cast<T>(value));                                                    \
        frames->Add(kf.GetPtr());                                                               \
        return true;                                                                            \
    }                                                                                           \
    extern "C" int32_t dm_noesis_animation_##SUFFIX##_keyframes_count(void* anim) {             \
        auto* a = cast<Noesis::ANIM>(anim);                                                     \
        if (!a) return -1;                                                                      \
        Noesis::COLL* frames = a->GetKeyFrames();                                               \
        return frames ? frames->Count() : 0;                                                    \
    }                                                                                           \
    extern "C" bool dm_noesis_animation_##SUFFIX##_keyframes_get_value(void* anim, int32_t idx, \
                                                                       ABIT* out) {             \
        auto* a = cast<Noesis::ANIM>(anim);                                                     \
        if (!a || !out) return false;                                                           \
        Noesis::COLL* frames = a->GetKeyFrames();                                               \
        if (!frames || idx < 0 || idx >= frames->Count()) return false;                         \
        *out = static_cast<ABIT>(frames->Get(static_cast<uint32_t>(idx))->GetValue());          \
        return true;                                                                            \
    }                                                                                           \
    extern "C" double dm_noesis_animation_##SUFFIX##_keyframes_get_key_time(void* anim,         \
                                                                           int32_t idx) {       \
        auto* a = cast<Noesis::ANIM>(anim);                                                     \
        if (!a) return -1.0;                                                                    \
        Noesis::COLL* frames = a->GetKeyFrames();                                               \
        if (!frames || idx < 0 || idx >= frames->Count()) return -1.0;                          \
        return frames->Get(static_cast<uint32_t>(idx))                                          \
            ->GetKeyTime()                                                                      \
            .GetTimeSpan()                                                                      \
            .GetTotalSeconds();                                                                 \
    }

DM_INT_KEYFRAMES(int16, Int16AnimationUsingKeyFrames, Int16KeyFrameCollection, Int16KeyFrame,
                 DiscreteInt16KeyFrame, LinearInt16KeyFrame, EasingInt16KeyFrame,
                 SplineInt16KeyFrame, int16_t, int32_t)
DM_INT_KEYFRAMES(int32, Int32AnimationUsingKeyFrames, Int32KeyFrameCollection, Int32KeyFrame,
                 DiscreteInt32KeyFrame, LinearInt32KeyFrame, EasingInt32KeyFrame,
                 SplineInt32KeyFrame, int32_t, int32_t)
DM_INT_KEYFRAMES(int64, Int64AnimationUsingKeyFrames, Int64KeyFrameCollection, Int64KeyFrame,
                 DiscreteInt64KeyFrame, LinearInt64KeyFrame, EasingInt64KeyFrame,
                 SplineInt64KeyFrame, int64_t, int64_t)

#undef DM_INT_KEYFRAMES

// ── Object key-frame animation ───────────────────────────────────────────────
//
// ObjectAnimationUsingKeyFrames has no From-To form and only discrete frames
// (an arbitrary BaseComponent can't be interpolated). The value crosses as a
// borrowed BaseComponent*; the collection takes its own reference. The value
// getter hands out a +1 reference (released by the caller).

extern "C" void* dm_noesis_animation_object_keyframes_create() {
    Noesis::Ptr<Noesis::ObjectAnimationUsingKeyFrames> a =
        *new Noesis::ObjectAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_object_keyframes_add(void* anim, double key_time_seconds,
                                                         void* value) {
    auto* a = cast<Noesis::ObjectAnimationUsingKeyFrames>(anim);
    if (!a) return false;
    Noesis::ObjectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::DiscreteObjectKeyFrame> kf = *new Noesis::DiscreteObjectKeyFrame();
    kf->SetValue(static_cast<Noesis::BaseComponent*>(value));
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_object_keyframes_count(void* anim) {
    auto* a = cast<Noesis::ObjectAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::ObjectKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" void* dm_noesis_animation_object_keyframes_get_value(void* anim, int32_t index) {
    auto* a = cast<Noesis::ObjectAnimationUsingKeyFrames>(anim);
    if (!a) return nullptr;
    Noesis::ObjectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return nullptr;
    return handout(frames->Get(static_cast<uint32_t>(index))->GetValue());
}

extern "C" double dm_noesis_animation_object_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::ObjectAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::ObjectKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── Boolean key-frame animation ───────────────────────────────────────────────
//
// BooleanAnimationUsingKeyFrames has no From-To form and only discrete frames (a
// bool can't be interpolated). The value crosses the ABI as a C bool.

extern "C" void* dm_noesis_animation_boolean_keyframes_create() {
    Noesis::Ptr<Noesis::BooleanAnimationUsingKeyFrames> a =
        *new Noesis::BooleanAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_boolean_keyframes_add(void* anim, double key_time_seconds,
                                                          bool value) {
    auto* a = cast<Noesis::BooleanAnimationUsingKeyFrames>(anim);
    if (!a) return false;
    Noesis::BooleanKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::DiscreteBooleanKeyFrame> kf = *new Noesis::DiscreteBooleanKeyFrame();
    kf->SetValue(value);
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_boolean_keyframes_count(void* anim) {
    auto* a = cast<Noesis::BooleanAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::BooleanKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

// Returns the frame value into *out; returns false on a bad handle / index.
extern "C" bool dm_noesis_animation_boolean_keyframes_get_value(void* anim, int32_t index,
                                                                bool* out) {
    auto* a = cast<Noesis::BooleanAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::BooleanKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    *out = frames->Get(static_cast<uint32_t>(index))->GetValue();
    return true;
}

extern "C" double dm_noesis_animation_boolean_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::BooleanAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::BooleanKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── String key-frame animation ────────────────────────────────────────────────
//
// StringAnimationUsingKeyFrames has no From-To form and only discrete frames (a
// string can't be interpolated). The value crosses as a NUL-terminated C string;
// the getter returns a pointer borrowed from the live key frame (copy it
// immediately) or NULL on a bad handle / index.

extern "C" void* dm_noesis_animation_string_keyframes_create() {
    Noesis::Ptr<Noesis::StringAnimationUsingKeyFrames> a =
        *new Noesis::StringAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_string_keyframes_add(void* anim, double key_time_seconds,
                                                         const char* value) {
    auto* a = cast<Noesis::StringAnimationUsingKeyFrames>(anim);
    if (!a || !value) return false;
    Noesis::StringKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::DiscreteStringKeyFrame> kf = *new Noesis::DiscreteStringKeyFrame();
    kf->SetValue(value);
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_string_keyframes_count(void* anim) {
    auto* a = cast<Noesis::StringAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::StringKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" const char* dm_noesis_animation_string_keyframes_get_value(void* anim, int32_t index) {
    auto* a = cast<Noesis::StringAnimationUsingKeyFrames>(anim);
    if (!a) return nullptr;
    Noesis::StringKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return nullptr;
    return frames->Get(static_cast<uint32_t>(index))->GetValue();
}

extern "C" double dm_noesis_animation_string_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::StringAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::StringKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── Matrix key-frame animation ───────────────────────────────────────────────
//
// MatrixAnimationUsingKeyFrames has no From-To form and only discrete frames (a
// matrix is not componentwise-interpolated). The matrix crosses as a 6-float
// {m00, m01, m10, m11, m20, m21} array (Noesis::Transform2 row layout).

extern "C" void* dm_noesis_animation_matrix_keyframes_create() {
    Noesis::Ptr<Noesis::MatrixAnimationUsingKeyFrames> a =
        *new Noesis::MatrixAnimationUsingKeyFrames();
    return handout(a.GetPtr());
}

extern "C" bool dm_noesis_animation_matrix_keyframes_add(void* anim, double key_time_seconds,
                                                         const float m[6]) {
    auto* a = cast<Noesis::MatrixAnimationUsingKeyFrames>(anim);
    if (!a || !m) return false;
    Noesis::MatrixKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames) return false;
    Noesis::Ptr<Noesis::DiscreteMatrixKeyFrame> kf = *new Noesis::DiscreteMatrixKeyFrame();
    kf->SetValue(Noesis::Transform2(m));
    kf->SetKeyTime(Noesis::KeyTime::FromTimeSpan(Noesis::TimeSpan(key_time_seconds)));
    frames->Add(kf.GetPtr());
    return true;
}

extern "C" int32_t dm_noesis_animation_matrix_keyframes_count(void* anim) {
    auto* a = cast<Noesis::MatrixAnimationUsingKeyFrames>(anim);
    if (!a) return -1;
    Noesis::MatrixKeyFrameCollection* frames = a->GetKeyFrames();
    return frames ? frames->Count() : 0;
}

extern "C" bool dm_noesis_animation_matrix_keyframes_get_value(void* anim, int32_t index,
                                                               float out[6]) {
    auto* a = cast<Noesis::MatrixAnimationUsingKeyFrames>(anim);
    if (!a || !out) return false;
    Noesis::MatrixKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return false;
    const Noesis::Transform2& t = frames->Get(static_cast<uint32_t>(index))->GetValue();
    const float* data = t.GetData();
    for (int i = 0; i < 6; ++i) out[i] = data[i];
    return true;
}

extern "C" double dm_noesis_animation_matrix_keyframes_get_key_time(void* anim, int32_t index) {
    auto* a = cast<Noesis::MatrixAnimationUsingKeyFrames>(anim);
    if (!a) return -1.0;
    Noesis::MatrixKeyFrameCollection* frames = a->GetKeyFrames();
    if (!frames || index < 0 || index >= frames->Count()) return -1.0;
    return frames->Get(static_cast<uint32_t>(index))->GetKeyTime().GetTimeSpan().GetTotalSeconds();
}

// ── KeySpline ────────────────────────────────────────────────────────────────
//
// The two Bezier control points that shape a spline key frame's progress curve.
// Points cross as {x, y} float[2].

extern "C" void* dm_noesis_animation_keyspline_create(float c1x, float c1y, float c2x, float c2y) {
    Noesis::Ptr<Noesis::KeySpline> k = *new Noesis::KeySpline(c1x, c1y, c2x, c2y);
    return handout(k.GetPtr());
}

extern "C" bool dm_noesis_animation_keyspline_set_control_point1(void* ks, float x, float y) {
    auto* k = cast<Noesis::KeySpline>(ks);
    if (!k) return false;
    k->SetControlPoint1(Noesis::Point(x, y));
    return true;
}

extern "C" bool dm_noesis_animation_keyspline_set_control_point2(void* ks, float x, float y) {
    auto* k = cast<Noesis::KeySpline>(ks);
    if (!k) return false;
    k->SetControlPoint2(Noesis::Point(x, y));
    return true;
}

extern "C" bool dm_noesis_animation_keyspline_get_control_point1(void* ks, float out[2]) {
    auto* k = cast<Noesis::KeySpline>(ks);
    if (!k || !out) return false;
    const Noesis::Point& p = k->GetControlPoint1();
    out[0] = p.x;
    out[1] = p.y;
    return true;
}

extern "C" bool dm_noesis_animation_keyspline_get_control_point2(void* ks, float out[2]) {
    auto* k = cast<Noesis::KeySpline>(ks);
    if (!k || !out) return false;
    const Noesis::Point& p = k->GetControlPoint2();
    out[0] = p.x;
    out[1] = p.y;
    return true;
}

// ── ParallelTimeline (timeline group) ─────────────────────────────────────────
//
// A code-built nestable timeline container. Children (any Timeline, including
// animations or nested ParallelTimelines) run in parallel off the group's clock.
// The children collection takes its own reference; the caller keeps ownership of
// each added child.

extern "C" void* dm_noesis_animation_parallel_timeline_create() {
    Noesis::Ptr<Noesis::ParallelTimeline> p = *new Noesis::ParallelTimeline();
    return handout(p.GetPtr());
}

extern "C" bool dm_noesis_animation_parallel_timeline_add_child(void* group, void* timeline) {
    auto* g = cast<Noesis::TimelineGroup>(group);
    auto* t = cast<Noesis::Timeline>(timeline);
    if (!g || !t) return false;
    Noesis::TimelineCollection* children = g->GetChildren();
    if (!children) {
        Noesis::Ptr<Noesis::TimelineCollection> created = *new Noesis::TimelineCollection();
        g->SetChildren(created.GetPtr());
        children = created.GetPtr();
    }
    children->Add(t);
    return true;
}

extern "C" int32_t dm_noesis_animation_parallel_timeline_child_count(void* group) {
    auto* g = cast<Noesis::TimelineGroup>(group);
    if (!g) return -1;
    Noesis::TimelineCollection* children = g->GetChildren();
    return children ? children->Count() : 0;
}

// ── BeginStoryboard (trigger action) ─────────────────────────────────────────
//
// A TriggerAction that begins a Storyboard with a chosen HandoffBehavior. Useful
// inside a trigger's actions; code-driven Storyboard::Begin covers the rest.

extern "C" void* dm_noesis_animation_begin_storyboard_create() {
    Noesis::Ptr<Noesis::BeginStoryboard> b = *new Noesis::BeginStoryboard();
    return handout(b.GetPtr());
}

extern "C" bool dm_noesis_animation_begin_storyboard_set_storyboard(void* bs, void* sb) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b) return false;
    b->SetStoryboard(cast<Noesis::Storyboard>(sb));
    return true;
}

extern "C" void* dm_noesis_animation_begin_storyboard_get_storyboard(void* bs) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b) return nullptr;
    return handout(b->GetStoryboard());
}

// behavior: matches Noesis::HandoffBehavior (0 SnapshotAndReplace, 1 Compose).
extern "C" bool dm_noesis_animation_begin_storyboard_set_handoff(void* bs, int32_t behavior) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b) return false;
    b->SetHandoffBehavior(static_cast<Noesis::HandoffBehavior>(behavior));
    return true;
}

extern "C" int32_t dm_noesis_animation_begin_storyboard_get_handoff(void* bs) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetHandoffBehavior());
}

extern "C" bool dm_noesis_animation_begin_storyboard_set_name(void* bs, const char* name) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b || !name) return false;
    b->SetName(name);
    return true;
}

extern "C" const char* dm_noesis_animation_begin_storyboard_get_name(void* bs) {
    auto* b = cast<Noesis::BeginStoryboard>(bs);
    if (!b) return nullptr;
    return b->GetName();
}

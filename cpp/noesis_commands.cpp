// ICommand-from-Rust bridge (TODO §4).
//
// A `RustCommand : Noesis::BaseCommand` trampoline lets XAML
// `Command="{Binding ...}"` invoke Rust logic. `BaseCommand` already
// implements the `ICommand` interface (CanExecute / Execute / the
// CanExecuteChanged EventHandler) and exposes `RaiseCanExecuteChanged()`;
// we only override CanExecute / Execute to forward into a Rust vtable and
// re-expose the raise so bound controls re-query (drives button
// enable/disable).
//
// Lifetime: unlike the synthetic-class / markup trampolines (which need a
// refcounted side `ClassData` because one registration backs many
// instances), a command is 1:1 with its Rust handler box. The box is owned
// directly by the `RustCommand` instance and freed in its destructor. The
// instance is an ordinary `BaseComponent`, so Noesis's intrusive refcount
// guarantees the destructor — and therefore the free handler — runs exactly
// once, after the LAST reference drops. That last reference may be the
// binding (Button.Command) holding the command alive well past the Rust
// `Command` handle being dropped, so CanExecute / Execute keep working until
// the visual tree lets go.

#include "noesis_shim.h"

#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsGui/BaseCommand.h>

namespace {

class RustCommand final: public Noesis::BaseCommand {
public:
    RustCommand(const dm_noesis_command_vtable* vt, void* userdata,
                dm_noesis_command_free_fn free_handler)
        : mVtable(*vt), mUserdata(userdata), mFree(free_handler) {}

    ~RustCommand() {
        // Donated ownership: the Rust handler box is dropped here, exactly
        // once, when the final BaseComponent reference goes away. Null the
        // pointer first so a (currently-impossible) re-entrant teardown
        // can't double-free.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    // From ICommand (via BaseCommand). `param` is the borrowed command
    // parameter BaseComponent* (may be null) — forwarded verbatim.
    bool CanExecute(Noesis::BaseComponent* param) const override {
        if (mVtable.can_execute) {
            return mVtable.can_execute(mUserdata, param);
        }
        return true;
    }

    void Execute(Noesis::BaseComponent* param) const override {
        if (mVtable.execute) {
            mVtable.execute(mUserdata, param);
        }
    }

    NS_IMPLEMENT_INLINE_REFLECTION(RustCommand, Noesis::BaseCommand, "DmNoesis.RustCommand") {}

private:
    dm_noesis_command_vtable  mVtable;
    void*                     mUserdata;
    dm_noesis_command_free_fn mFree;
};

}  // namespace

// ── C ABI surface ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_command_create(
    const dm_noesis_command_vtable* vt,
    void* userdata,
    dm_noesis_command_free_fn free_handler) {
    if (!vt) return nullptr;
    // BaseRefCounted starts at refcount 1 — that initial reference IS the
    // caller's +1, balanced by dm_noesis_command_destroy. (No AddReference:
    // a binding that later stores the command takes its own ref via
    // SetValueObject, so the handler box outlives our destroy until that ref
    // also drops.)
    auto* cmd = new RustCommand(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void dm_noesis_command_destroy(void* command) {
    if (!command) return;
    static_cast<Noesis::BaseComponent*>(command)->Release();
}

extern "C" void dm_noesis_command_raise_can_execute_changed(void* command) {
    if (!command) return;
    auto* cmd = Noesis::DynamicCast<Noesis::BaseCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (cmd) {
        cmd->RaiseCanExecuteChanged();
    }
}

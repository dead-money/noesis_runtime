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

#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/Symbol.h>
#include <NsCore/Type.h>
#include <NsCore/TypeClass.h>
#include <NsGui/ApplicationCommands.h>
#include <NsGui/BaseCommand.h>
#include <NsGui/CommandBinding.h>
#include <NsGui/ComponentCommands.h>
#include <NsGui/RoutedCommand.h>
#include <NsGui/RoutedUICommand.h>
#include <NsGui/UICollection.h>
#include <NsGui/UIElement.h>

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

// ── RoutedCommand / RoutedUICommand (TODO §4) ───────────────────────────────
//
// A RoutedCommand routes Execute / CanExecute through the element tree to the
// first matching CommandBinding (below). Construction needs an owner TypeClass;
// we resolve it from a type name through the Core reflection registry (a
// built-in like "UIElement" or a §9-registered custom class). Both are
// BaseCommand-derived, so dm_noesis_command_raise_can_execute_changed works on
// them too. Returned commands carry +1 (release via
// dm_noesis_base_component_release).

namespace {
const Noesis::TypeClass* resolve_owner(const char* owner_type_name) {
    if (!owner_type_name) return nullptr;
    const Noesis::Type* t = Noesis::Reflection::GetType(Noesis::Symbol(owner_type_name));
    return Noesis::DynamicCast<const Noesis::TypeClass*>(t);
}
}  // namespace

extern "C" void* dm_noesis_routed_command_create(const char* name, const char* owner_type_name) {
    if (!name) return nullptr;
    const Noesis::TypeClass* owner = resolve_owner(owner_type_name);
    if (!owner) return nullptr;
    // BaseRefCounted starts at refcount 1 — that initial ref is the caller's +1.
    auto* cmd = new Noesis::RoutedCommand(Noesis::Symbol(name), owner);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void* dm_noesis_routed_ui_command_create(
    const char* name, const char* text, const char* owner_type_name) {
    if (!name) return nullptr;
    const Noesis::TypeClass* owner = resolve_owner(owner_type_name);
    if (!owner) return nullptr;
    auto* cmd = new Noesis::RoutedUICommand(text ? text : "", Noesis::Symbol(name), owner);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void dm_noesis_routed_command_execute(void* command, void* param, void* target) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(target));
    if (cmd && ui) {
        cmd->Execute(static_cast<Noesis::BaseComponent*>(param), ui);
    }
}

extern "C" bool dm_noesis_routed_command_can_execute(void* command, void* param, void* target) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(target));
    if (cmd && ui) {
        return cmd->CanExecute(static_cast<Noesis::BaseComponent*>(param), ui);
    }
    return false;
}

// Registered name (RoutedCommand::GetName), borrowed (interned Symbol string).
extern "C" const char* dm_noesis_routed_command_get_name(void* command) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    return cmd ? cmd->GetName().Str() : nullptr;
}

extern "C" const char* dm_noesis_routed_ui_command_get_text(void* command) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedUICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    return cmd ? cmd->GetText() : nullptr;
}

extern "C" void dm_noesis_routed_ui_command_set_text(void* command, const char* text) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedUICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (cmd) {
        cmd->SetText(text ? text : "");
    }
}

// ── CommandBinding (TODO §4) ────────────────────────────────────────────────
//
// Binds a command to Rust Executed / CanExecute handlers and attaches to an
// element's CommandBindings, so an invoked RoutedCommand (or built-in) routing
// through that element fires the handler. Lifetime mirrors the routed-event
// bridges (noesis_events.cpp): a heap RustCommandBinding owns the donated Rust
// box + a +1 on the CommandBinding, registers the delegates with `+=`, and
// detaches with `-=` in its destructor.

namespace {

class RustCommandBinding {
public:
    RustCommandBinding(Noesis::ICommand* command,
                       dm_noesis_cmd_executed_fn executed,
                       dm_noesis_cmd_can_execute_fn can_execute,
                       void* userdata, dm_noesis_command_free_fn free_handler)
        : mExecuted(executed), mCanExecute(can_execute), mUserdata(userdata),
          mFree(free_handler) {
        mBinding = *new Noesis::CommandBinding(command);
        mBinding->Executed() += Noesis::MakeDelegate(this, &RustCommandBinding::OnExecuted);
        mBinding->CanExecute() += Noesis::MakeDelegate(this, &RustCommandBinding::OnCanExecute);
    }

    ~RustCommandBinding() {
        mBinding->Executed() -= Noesis::MakeDelegate(this, &RustCommandBinding::OnExecuted);
        mBinding->CanExecute() -= Noesis::MakeDelegate(this, &RustCommandBinding::OnCanExecute);
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
        // Drop our +1 on the CommandBinding (the element's collection, if
        // attached, holds its own ref and keeps it alive as needed).
        mBinding.Reset();
    }

    RustCommandBinding(const RustCommandBinding&) = delete;
    RustCommandBinding& operator=(const RustCommandBinding&) = delete;

    Noesis::CommandBinding* binding() const { return mBinding; }

private:
    void OnExecuted(Noesis::BaseComponent*, const Noesis::ExecutedRoutedEventArgs& args) {
        if (mExecuted) {
            mExecuted(mUserdata, args.parameter);
        }
        args.handled = true;
    }

    void OnCanExecute(Noesis::BaseComponent*, const Noesis::CanExecuteRoutedEventArgs& args) {
        bool can = true;
        if (mCanExecute) {
            can = mCanExecute(mUserdata, args.parameter);
        }
        args.canExecute = can;
        args.handled = true;
    }

    Noesis::Ptr<Noesis::CommandBinding> mBinding;
    dm_noesis_cmd_executed_fn    mExecuted;
    dm_noesis_cmd_can_execute_fn mCanExecute;
    void*                        mUserdata;
    dm_noesis_command_free_fn    mFree;
};

}  // namespace

extern "C" void* dm_noesis_command_binding_create(
    void* command, dm_noesis_cmd_executed_fn executed,
    dm_noesis_cmd_can_execute_fn can_execute, void* userdata,
    dm_noesis_command_free_fn free_handler) {
    if (!command) return nullptr;
    auto* cmd = Noesis::DynamicCast<Noesis::ICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (!cmd) return nullptr;
    return new RustCommandBinding(cmd, executed, can_execute, userdata, free_handler);
}

extern "C" bool dm_noesis_command_binding_attach(void* token, void* element) {
    if (!token || !element) return false;
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ui) return false;
    auto* bridge = static_cast<RustCommandBinding*>(token);
    ui->GetCommandBindings()->Add(bridge->binding());
    return true;
}

extern "C" void dm_noesis_command_binding_destroy(void* token) {
    if (!token) return;
    // Detaches the delegates, frees the donated box, drops our binding ref.
    delete static_cast<RustCommandBinding*>(token);
}

// ── Built-in command libraries (TODO §4) ────────────────────────────────────
//
// Borrowed `const RoutedUICommand*` singletons owned by the framework — do NOT
// release. Indexed by the enums in src/commands.rs; the switch evaluates each
// static at call time (after GUI init), so the pointers are live. NULL on an
// out-of-range index.

extern "C" const void* dm_noesis_application_command(uint32_t which) {
    using AC = Noesis::ApplicationCommands;
    switch (which) {
        case 0:  return AC::CancelPrintCommand;
        case 1:  return AC::CloseCommand;
        case 2:  return AC::ContextMenuCommand;
        case 3:  return AC::CopyCommand;
        case 4:  return AC::CorrectionListCommand;
        case 5:  return AC::CutCommand;
        case 6:  return AC::DeleteCommand;
        case 7:  return AC::FindCommand;
        case 8:  return AC::HelpCommand;
        case 9:  return AC::NewCommand;
        case 10: return AC::OpenCommand;
        case 11: return AC::PasteCommand;
        case 12: return AC::PrintCommand;
        case 13: return AC::PrintPreviewCommand;
        case 14: return AC::PropertiesCommand;
        case 15: return AC::RedoCommand;
        case 16: return AC::ReplaceCommand;
        case 17: return AC::SaveCommand;
        case 18: return AC::SaveAsCommand;
        case 19: return AC::SelectAllCommand;
        case 20: return AC::StopCommand;
        case 21: return AC::UndoCommand;
        default: return nullptr;
    }
}

extern "C" const void* dm_noesis_component_command(uint32_t which) {
    using CC = Noesis::ComponentCommands;
    switch (which) {
        case 0:  return CC::ExtendSelectionDownCommand;
        case 1:  return CC::ExtendSelectionLeftCommand;
        case 2:  return CC::ExtendSelectionRightCommand;
        case 3:  return CC::ExtendSelectionUpCommand;
        case 4:  return CC::MoveDownCommand;
        case 5:  return CC::MoveFocusBackCommand;
        case 6:  return CC::MoveFocusDownCommand;
        case 7:  return CC::MoveFocusForwardCommand;
        case 8:  return CC::MoveFocusPageDownCommand;
        case 9:  return CC::MoveFocusPageUpCommand;
        case 10: return CC::MoveFocusUpCommand;
        case 11: return CC::MoveLeftCommand;
        case 12: return CC::MoveRightCommand;
        case 13: return CC::MoveToEndCommand;
        case 14: return CC::MoveToHomeCommand;
        case 15: return CC::MoveToPageDownCommand;
        case 16: return CC::MoveToPageUpCommand;
        case 17: return CC::MoveUpCommand;
        case 18: return CC::ScrollByLineCommand;
        case 19: return CC::ScrollPageDownCommand;
        case 20: return CC::ScrollPageLeftCommand;
        case 21: return CC::ScrollPageRightCommand;
        case 22: return CC::ScrollPageUpCommand;
        case 23: return CC::SelectToEndCommand;
        case 24: return CC::SelectToHomeCommand;
        case 25: return CC::SelectToPageDownCommand;
        case 26: return CC::SelectToPageUpCommand;
        default: return nullptr;
    }
}

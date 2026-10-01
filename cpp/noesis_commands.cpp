// Commands: a Rust-backed ICommand, RoutedCommand / RoutedUICommand,
// CommandBinding, and the built-in Application/Component command libraries.
//
// RustCommand owns its Rust handler box and frees it in the destructor, which
// runs once when the last reference drops. A binding (Button.Command) may hold
// that reference after the Rust `Command` handle is gone, so CanExecute /
// Execute keep working until the visual tree lets go.

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
    RustCommand(const noesis_command_vtable* vt, void* userdata,
                noesis_command_free_fn free_handler)
        : mVtable(*vt), mUserdata(userdata), mFree(free_handler) {}

    ~RustCommand() {
        // Null first so a re-entrant teardown can't double-free.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    // `param` is borrowed and may be null.
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
    noesis_command_vtable  mVtable;
    void*                     mUserdata;
    noesis_command_free_fn mFree;
};

}  // namespace

extern "C" void* noesis_command_create(
    const noesis_command_vtable* vt,
    void* userdata,
    noesis_command_free_fn free_handler) {
    if (!vt) return nullptr;
    // The initial refcount of 1 is the caller's +1, balanced by
    // noesis_command_destroy.
    auto* cmd = new RustCommand(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void noesis_command_destroy(void* command) {
    if (!command) return;
    static_cast<Noesis::BaseComponent*>(command)->Release();
}

extern "C" void noesis_command_raise_can_execute_changed(void* command) {
    if (!command) return;
    auto* cmd = Noesis::DynamicCast<Noesis::BaseCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (cmd) {
        cmd->RaiseCanExecuteChanged();
    }
}

// ── RoutedCommand / RoutedUICommand ─────────────────────────────────────────
//
// The owner TypeClass is resolved by name through reflection (a built-in like
// "UIElement" or a registered custom class); an unknown name returns null.
// Returned commands carry +1, released via noesis_base_component_release.

namespace {
const Noesis::TypeClass* resolve_owner(const char* owner_type_name) {
    if (!owner_type_name) return nullptr;
    const Noesis::Type* t = Noesis::Reflection::GetType(Noesis::Symbol(owner_type_name));
    return Noesis::DynamicCast<const Noesis::TypeClass*>(t);
}
}  // namespace

extern "C" void* noesis_routed_command_create(const char* name, const char* owner_type_name) {
    if (!name) return nullptr;
    const Noesis::TypeClass* owner = resolve_owner(owner_type_name);
    if (!owner) return nullptr;
    // Initial refcount of 1 is the caller's +1.
    auto* cmd = new Noesis::RoutedCommand(Noesis::Symbol(name), owner);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void* noesis_routed_ui_command_create(
    const char* name, const char* text, const char* owner_type_name) {
    if (!name) return nullptr;
    const Noesis::TypeClass* owner = resolve_owner(owner_type_name);
    if (!owner) return nullptr;
    auto* cmd = new Noesis::RoutedUICommand(text ? text : "", Noesis::Symbol(name), owner);
    return static_cast<Noesis::BaseComponent*>(cmd);
}

extern "C" void noesis_routed_command_execute(void* command, void* param, void* target) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(target));
    if (cmd && ui) {
        cmd->Execute(static_cast<Noesis::BaseComponent*>(param), ui);
    }
}

extern "C" bool noesis_routed_command_can_execute(void* command, void* param, void* target) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(target));
    if (cmd && ui) {
        return cmd->CanExecute(static_cast<Noesis::BaseComponent*>(param), ui);
    }
    return false;
}

// Borrowed; interned Symbol string.
extern "C" const char* noesis_routed_command_get_name(void* command) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedCommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    return cmd ? cmd->GetName().Str() : nullptr;
}

extern "C" const char* noesis_routed_ui_command_get_text(void* command) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedUICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    return cmd ? cmd->GetText() : nullptr;
}

extern "C" void noesis_routed_ui_command_set_text(void* command, const char* text) {
    auto* cmd = Noesis::DynamicCast<Noesis::RoutedUICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (cmd) {
        cmd->SetText(text ? text : "");
    }
}

// ── CommandBinding ───────────────────────────────────────────────────────────
//
// A heap RustCommandBinding owns the donated Rust box and a +1 on the
// CommandBinding; its destructor unhooks the delegates and detaches the binding
// from the element it was attached to.

namespace {

class RustCommandBinding {
public:
    RustCommandBinding(Noesis::ICommand* command,
                       noesis_cmd_executed_fn executed,
                       noesis_cmd_can_execute_fn can_execute,
                       void* userdata, noesis_command_free_fn free_handler)
        : mExecuted(executed), mCanExecute(can_execute), mUserdata(userdata),
          mFree(free_handler) {
        mBinding = *new Noesis::CommandBinding(command);
        mBinding->Executed() += Noesis::MakeDelegate(this, &RustCommandBinding::OnExecuted);
        mBinding->CanExecute() += Noesis::MakeDelegate(this, &RustCommandBinding::OnCanExecute);
    }

    ~RustCommandBinding() {
        mBinding->Executed() -= Noesis::MakeDelegate(this, &RustCommandBinding::OnExecuted);
        mBinding->CanExecute() -= Noesis::MakeDelegate(this, &RustCommandBinding::OnCanExecute);
        // Otherwise inert bindings accumulate for the element's lifetime.
        if (mAttached) {
            if (auto* bindings = mAttached->GetCommandBindings()) {
                bindings->Remove(mBinding);
            }
            mAttached.Reset();
        }
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
        mBinding.Reset();
    }

    RustCommandBinding(const RustCommandBinding&) = delete;
    RustCommandBinding& operator=(const RustCommandBinding&) = delete;

    Noesis::CommandBinding* binding() const { return mBinding; }

    void setAttached(Noesis::UIElement* element) {
        mAttached.Reset(element);
    }

    // True: a callback is on the stack, so the outermost dispatch frame deletes
    // and the caller must not. Rust may drop the binding from inside its own
    // callback. A depth counter because a callback may re-invoke the command.
    // View thread only, so no atomics.
    bool deferDeleteIfDispatching() {
        if (mDispatchDepth > 0) {
            mPendingDelete = true;
            return true;
        }
        return false;
    }

private:
    void OnExecuted(Noesis::BaseComponent*, const Noesis::ExecutedRoutedEventArgs& args) {
        mDispatchDepth++;
        if (mExecuted) {
            mExecuted(mUserdata, args.parameter);
        }
        args.handled = true;
        if (--mDispatchDepth == 0 && mPendingDelete) {
            delete this;
        }
    }

    void OnCanExecute(Noesis::BaseComponent*, const Noesis::CanExecuteRoutedEventArgs& args) {
        mDispatchDepth++;
        bool can = true;
        if (mCanExecute) {
            can = mCanExecute(mUserdata, args.parameter);
        }
        args.canExecute = can;
        args.handled = true;
        if (--mDispatchDepth == 0 && mPendingDelete) {
            delete this;
        }
    }

    Noesis::Ptr<Noesis::CommandBinding> mBinding;
    Noesis::Ptr<Noesis::UIElement> mAttached;
    noesis_cmd_executed_fn    mExecuted;
    noesis_cmd_can_execute_fn mCanExecute;
    void*                        mUserdata;
    noesis_command_free_fn    mFree;
    uint32_t mDispatchDepth = 0;
    bool mPendingDelete = false;
};

}  // namespace

extern "C" void* noesis_command_binding_create(
    void* command, noesis_cmd_executed_fn executed,
    noesis_cmd_can_execute_fn can_execute, void* userdata,
    noesis_command_free_fn free_handler) {
    if (!command) return nullptr;
    auto* cmd = Noesis::DynamicCast<Noesis::ICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (!cmd) return nullptr;
    return new RustCommandBinding(cmd, executed, can_execute, userdata, free_handler);
}

extern "C" bool noesis_command_binding_attach(void* token, void* element) {
    if (!token || !element) return false;
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ui) return false;
    auto* bridge = static_cast<RustCommandBinding*>(token);
    ui->GetCommandBindings()->Add(bridge->binding());
    bridge->setAttached(ui);
    return true;
}

extern "C" void noesis_command_binding_destroy(void* token) {
    if (!token) return;
    auto* bridge = static_cast<RustCommandBinding*>(token);
    if (bridge->deferDeleteIfDispatching()) return;
    delete bridge;
}

// ── Built-in command libraries ───────────────────────────────────────────────
//
// Borrowed framework singletons; never release. Indexed by the enums in
// src/commands.rs; null when out of range. Each static is read at call time,
// after GUI init.

extern "C" const void* noesis_application_command(uint32_t which) {
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

extern "C" const void* noesis_component_command(uint32_t which) {
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

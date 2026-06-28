#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Init.h>
#include <NsCore/Log.h>
#include <NsCore/Version.h>
#include <NsGui/IntegrationAPI.h>

namespace {

dm_noesis_log_fn g_log_cb       = nullptr;
void*            g_log_userdata = nullptr;

void log_trampoline(const char* file, uint32_t line, uint32_t level,
                    const char* channel, const char* message)
{
    if (g_log_cb) {
        g_log_cb(g_log_userdata, file, line,
                 static_cast<dm_noesis_log_level>(level),
                 channel ? channel : "",
                 message ? message : "");
    }
}

}  // namespace

extern "C" void dm_noesis_set_license(const char* name, const char* key)
{
    Noesis::SetLicense(name ? name : "", key ? key : "");
}

extern "C" void dm_noesis_set_log_handler(dm_noesis_log_fn cb, void* userdata)
{
    g_log_cb       = cb;
    g_log_userdata = userdata;
    Noesis::SetLogHandler(cb ? log_trampoline : nullptr);
}

// ── Inspector / hot-reload toggles + queries (TODO §17) ─────────────────────
//
// All GUI:: free calls. The Disable* trio MUST run before GUI::Init (i.e.
// before dm_noesis_init); the query + pump are runtime calls. On a Release
// dylib the Inspector is compiled out, so these degrade to no-ops /
// always-false — see the header for the full reality check.

extern "C" void dm_noesis_disable_hot_reload(void)
{
    Noesis::GUI::DisableHotReload();
}

extern "C" void dm_noesis_disable_socket_init(void)
{
    Noesis::GUI::DisableSocketInit();
}

extern "C" void dm_noesis_disable_inspector(void)
{
    Noesis::GUI::DisableInspector();
}

extern "C" bool dm_noesis_is_inspector_connected(void)
{
    return Noesis::GUI::IsInspectorConnected();
}

extern "C" void dm_noesis_update_inspector(void)
{
    Noesis::GUI::UpdateInspector();
}

extern "C" void dm_noesis_init(void)
{
    Noesis::Init();
}

// Forward declarations for the per-subsystem shutdown sweeps. Defined in
// noesis_classes.cpp / noesis_markup.cpp respectively.
extern "C" void dm_noesis_classes_force_free_at_shutdown(void);
extern "C" void dm_noesis_markup_extensions_force_free_at_shutdown(void);

extern "C" void dm_noesis_shutdown(void)
{
    // Order matters: Noesis::Shutdown must run first to destroy every
    // live DependencyObject (which fires their refcount-driven Release
    // calls into our trampolines, naturally freeing most handler boxes).
    // The sweeps then defensively free any handler boxes whose owning
    // instances bypassed normal teardown — a belt-and-suspenders for
    // orphaned-View paths that never `drop`-ed before shutdown.
    Noesis::Shutdown();
    dm_noesis_classes_force_free_at_shutdown();
    dm_noesis_markup_extensions_force_free_at_shutdown();
}

extern "C" const char* dm_noesis_version(void)
{
    return Noesis::GetBuildVersion();
}

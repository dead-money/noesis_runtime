#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Init.h>
#include <NsCore/Log.h>
#include <NsCore/Version.h>
#include <NsGui/IntegrationAPI.h>

namespace {

noesis_log_fn g_log_cb       = nullptr;
void*            g_log_userdata = nullptr;

void log_trampoline(const char* file, uint32_t line, uint32_t level,
                    const char* channel, const char* message)
{
    if (g_log_cb) {
        g_log_cb(g_log_userdata, file, line,
                 static_cast<noesis_log_level>(level),
                 channel ? channel : "",
                 message ? message : "");
    }
}

}  // namespace

extern "C" void noesis_set_license(const char* name, const char* key)
{
    Noesis::SetLicense(name ? name : "", key ? key : "");
}

extern "C" void noesis_set_log_handler(noesis_log_fn cb, void* userdata)
{
    g_log_cb       = cb;
    g_log_userdata = userdata;
    Noesis::SetLogHandler(cb ? log_trampoline : nullptr);
}

// The Disable* calls only take effect before noesis_init. Release SDK builds
// compile the Inspector out, so these are no-ops / always-false there.

extern "C" void noesis_disable_hot_reload(void)
{
    Noesis::GUI::DisableHotReload();
}

extern "C" void noesis_disable_socket_init(void)
{
    Noesis::GUI::DisableSocketInit();
}

extern "C" void noesis_disable_inspector(void)
{
    Noesis::GUI::DisableInspector();
}

extern "C" bool noesis_is_inspector_connected(void)
{
    return Noesis::GUI::IsInspectorConnected();
}

extern "C" void noesis_update_inspector(void)
{
    Noesis::GUI::UpdateInspector();
}

extern "C" void noesis_init(void)
{
    Noesis::Init();
}

// Defined in noesis_classes.cpp, noesis_markup.cpp and noesis_plain_vm.cpp.
extern "C" void noesis_classes_force_free_at_shutdown(void);
extern "C" void noesis_markup_extensions_force_free_at_shutdown(void);
extern "C" void noesis_plain_vm_force_free_at_shutdown(void);

extern "C" void noesis_shutdown(void)
{
    // Noesis::Shutdown first: destroying the remaining objects frees most
    // handler boxes through the normal release path. The sweeps then free
    // boxes whose instances were never released (e.g. a leaked View).
    Noesis::Shutdown();
    noesis_classes_force_free_at_shutdown();
    noesis_markup_extensions_force_free_at_shutdown();
    noesis_plain_vm_force_free_at_shutdown();
}

extern "C" const char* noesis_version(void)
{
    return Noesis::GetBuildVersion();
}

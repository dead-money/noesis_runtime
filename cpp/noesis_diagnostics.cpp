// Diagnostics shim: error / assert handlers, memory-usage queries. All of these
// are NsCore kernel functions. The kernel must be up (GUI::Init / noesis_init
// has run) before they do anything meaningful.
//
// Two handler shapes:
//
//   * SetErrorHandler / SetAssertHandler take a BARE C function pointer with no
//     userdata, so the Rust callback lives in a process-global slot here. A
//     fixed trampoline reads the slot and forwards to (cb, userdata). The very
//     first install captures the Noesis default handler so clearing restores it
//     (mirrors the house "restore the PREVIOUS handler on Drop" convention).
//     Each setter also returns the previous (cb, userdata) pair via out-params
//     so a Rust RAII guard can restore a nested predecessor.
//
//   * SetThreadErrorHandler / ErrorHandler2 carries a void* user, so the boxed
//     Rust closure threads straight through it, no global slot needed. The
//     C struct noesis_error_context is binary-compatible with
//     Noesis::ErrorContext, and noesis_error2_fn with Noesis::ErrorHandler2,
//     so we reinterpret_cast across the ABI (static_assert'd below).
//
// The Invoke* entrypoints wrap the SDK's public invokers so Rust tests can
// drive the registered handlers through the real Noesis dispatch path. Always
// drive with fatal=false: a fatal error or a failed NS_ASSERT can abort.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Error.h>
#include <NsCore/Memory.h>

// noesis_error_context must mirror Noesis::ErrorContext bit-for-bit so we can
// hand the same pointer to both sides of the ABI.
static_assert(sizeof(noesis_error_context) == sizeof(Noesis::ErrorContext),
              "noesis_error_context layout drift vs Noesis::ErrorContext");
static_assert(offsetof(noesis_error_context, uri) == offsetof(Noesis::ErrorContext, uri),
              "noesis_error_context::uri offset drift");
static_assert(offsetof(noesis_error_context, line) == offsetof(Noesis::ErrorContext, line),
              "noesis_error_context::line offset drift");
static_assert(offsetof(noesis_error_context, column) == offsetof(Noesis::ErrorContext, column),
              "noesis_error_context::column offset drift");

namespace {

// ── Global error-handler slot (SetErrorHandler has no userdata) ──────────────
noesis_error_fn   g_error_cb        = nullptr;
void*                g_error_user      = nullptr;
Noesis::ErrorHandler g_saved_error     = nullptr;  // Noesis handler we displaced
bool                 g_error_installed = false;

void error_trampoline(const char* file, uint32_t line, const char* message, bool fatal)
{
    if (g_error_cb) {
        g_error_cb(g_error_user, file ? file : "", line, message ? message : "", fatal);
    }
}

// ── Global assert-handler slot (SetAssertHandler has no userdata) ────────────
noesis_assert_fn   g_assert_cb        = nullptr;
void*                 g_assert_user      = nullptr;
Noesis::AssertHandler g_saved_assert     = nullptr;
bool                  g_assert_installed = false;

bool assert_trampoline(const char* file, uint32_t line, const char* expr)
{
    if (g_assert_cb) {
        return g_assert_cb(g_assert_user, file ? file : "", line, expr ? expr : "");
    }
    return false;
}

}  // namespace

// ── Error handler (global, no userdata) ──────────────────────────────────────

extern "C" void noesis_set_error_handler(noesis_error_fn cb, void* userdata,
    noesis_error_fn* out_prev_cb, void** out_prev_user)
{
    if (out_prev_cb)   *out_prev_cb   = g_error_cb;
    if (out_prev_user) *out_prev_user = g_error_user;

    g_error_cb   = cb;
    g_error_user = userdata;

    if (cb) {
        if (!g_error_installed) {
            g_saved_error = Noesis::SetErrorHandler(error_trampoline);
            g_error_installed = true;
        }
    } else if (g_error_installed) {
        Noesis::SetErrorHandler(g_saved_error);
        g_error_installed = false;
        g_saved_error = nullptr;
    }
}

// ── Assert handler (global, no userdata) ──────────────────────────────────────

extern "C" void noesis_set_assert_handler(noesis_assert_fn cb, void* userdata,
    noesis_assert_fn* out_prev_cb, void** out_prev_user)
{
    if (out_prev_cb)   *out_prev_cb   = g_assert_cb;
    if (out_prev_user) *out_prev_user = g_assert_user;

    g_assert_cb   = cb;
    g_assert_user = userdata;

    if (cb) {
        if (!g_assert_installed) {
            g_saved_assert = Noesis::SetAssertHandler(assert_trampoline);
            g_assert_installed = true;
        }
    } else if (g_assert_installed) {
        Noesis::SetAssertHandler(g_saved_assert);
        g_assert_installed = false;
        g_saved_assert = nullptr;
    }
}

// ── Thread error handler (ErrorHandler2, carries userdata) ────────────────────

extern "C" void noesis_set_thread_error_handler(noesis_error2_fn handler, void* userdata,
    noesis_error2_fn* out_prev_handler, void** out_prev_user)
{
    // noesis_error2_fn and noesis_error_context are layout-compatible with
    // Noesis::ErrorHandler2 / Noesis::ErrorContext (static_assert'd above), so a
    // reinterpret_cast is sound in both directions.
    Noesis::ErrorHandlerData prev = Noesis::SetThreadErrorHandler(
        userdata, reinterpret_cast<Noesis::ErrorHandler2>(handler));

    if (out_prev_handler) {
        *out_prev_handler = reinterpret_cast<noesis_error2_fn>(prev.handler);
    }
    if (out_prev_user) {
        *out_prev_user = prev.user;
    }
}

// ── Invokers (drive the registered handlers through the real SDK dispatch) ────

extern "C" void noesis_invoke_error_handler(const char* file, uint32_t line, bool fatal,
    bool has_context, const char* uri, uint32_t ctx_line, uint32_t ctx_col, const char* message)
{
    // Pass `message` through a "%s" format so an arbitrary message string is
    // never interpreted as a printf format.
    if (has_context) {
        Noesis::ErrorContext ctx{uri, ctx_line, ctx_col};
        Noesis::InvokeErrorHandler(file, line, fatal, &ctx, "%s", message ? message : "");
    } else {
        Noesis::InvokeErrorHandler(file, line, fatal, nullptr, "%s", message ? message : "");
    }
}

extern "C" bool noesis_invoke_assert_handler(const char* file, uint32_t line, const char* expr)
{
    return Noesis::InvokeAssertHandler(file, line, expr);
}

// ── Memory-usage queries ─────────────────────────────────────────────────────

extern "C" uint32_t noesis_get_allocated_memory(void)
{
    return Noesis::GetAllocatedMemory();
}

extern "C" uint32_t noesis_get_allocated_memory_accum(void)
{
    return Noesis::GetAllocatedMemoryAccum();
}

extern "C" uint32_t noesis_get_allocations_count(void)
{
    return Noesis::GetAllocationsCount();
}

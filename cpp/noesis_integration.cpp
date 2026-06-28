// C ABI shim for Noesis's system integration callbacks (Section 14).
//
//   `NsGui/IntegrationAPI.h` (namespace Noesis::GUI) exposes a handful of
//   process-global host hooks, each registered as a `(void* user, callback)`
//   pair, plus the `OpenUrl` / `PlayAudio` triggers and `SetCulture` /
//   `GetCulture`.
//
//   For the callback hooks we keep a static `(user, cb)` slot per hook and
//   register a C++ trampoline whose job is to translate the Noesis-typed
//   arguments — `Cursor*` (→ CursorType int), `const Uri&` (→ const char*) —
//   into the plain C ABI the Rust side declared. The Rust user pointer is
//   forwarded untouched. Passing a NULL `cb` clears the slot and the
//   underlying Noesis callback.
//
//   These are single, process-global registrations (Noesis stores exactly
//   one `(user, callback)` per hook), so static storage is the natural fit;
//   no per-instance allocation is needed.

#include <cstdint>
#include <string>

#include <NsCore/CultureInfo.h>
#include <NsGui/Cursor.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/Uri.h>

#include "noesis_shim.h"

namespace {

// ── Callback slots ─────────────────────────────────────────────────────────

struct CursorReg { void* user; noesis_cursor_cb cb; };
struct KeyboardReg { void* user; noesis_software_keyboard_cb cb; };
struct OpenUrlReg { void* user; noesis_open_url_cb cb; };
struct PlayAudioReg { void* user; noesis_play_audio_cb cb; };

CursorReg g_cursor{nullptr, nullptr};
KeyboardReg g_keyboard{nullptr, nullptr};
OpenUrlReg g_openUrl{nullptr, nullptr};
PlayAudioReg g_playAudio{nullptr, nullptr};

// ── Trampolines (Noesis signature → C ABI) ──────────────────────────────────

void CursorTramp(void* user, Noesis::IView* view, Noesis::Cursor* cursor) {
    auto* reg = static_cast<CursorReg*>(user);
    if (!reg || !reg->cb) return;
    int32_t type = cursor ? static_cast<int32_t>(cursor->Type())
                          : static_cast<int32_t>(Noesis::CursorType_None);
    reg->cb(reg->user, static_cast<void*>(view), type);
}

void KeyboardTramp(void* user, Noesis::UIElement* focused, bool open) {
    auto* reg = static_cast<KeyboardReg*>(user);
    if (!reg || !reg->cb) return;
    reg->cb(reg->user, static_cast<void*>(focused), open);
}

void OpenUrlTramp(void* user, const char* url) {
    auto* reg = static_cast<OpenUrlReg*>(user);
    if (!reg || !reg->cb) return;
    reg->cb(reg->user, url ? url : "");
}

void PlayAudioTramp(void* user, const Noesis::Uri& uri, float volume) {
    auto* reg = static_cast<PlayAudioReg*>(user);
    if (!reg || !reg->cb) return;
    const char* str = uri.Str();
    reg->cb(reg->user, str ? str : "", volume);
}

}  // namespace

// ── Cursor ──────────────────────────────────────────────────────────────────

extern "C" void noesis_set_cursor_callback(void* user, noesis_cursor_cb cb) {
    if (cb) {
        g_cursor.user = user;
        g_cursor.cb = cb;
        Noesis::GUI::SetCursorCallback(&g_cursor, CursorTramp);
    } else {
        g_cursor.user = nullptr;
        g_cursor.cb = nullptr;
        Noesis::GUI::SetCursorCallback(nullptr, nullptr);
    }
}

// ── Software keyboard ────────────────────────────────────────────────────────

extern "C" void noesis_set_software_keyboard_callback(
    void* user, noesis_software_keyboard_cb cb)
{
    if (cb) {
        g_keyboard.user = user;
        g_keyboard.cb = cb;
        Noesis::GUI::SetSoftwareKeyboardCallback(&g_keyboard, KeyboardTramp);
    } else {
        g_keyboard.user = nullptr;
        g_keyboard.cb = nullptr;
        Noesis::GUI::SetSoftwareKeyboardCallback(nullptr, nullptr);
    }
}

// ── Open URL ─────────────────────────────────────────────────────────────────

extern "C" void noesis_set_open_url_callback(void* user, noesis_open_url_cb cb) {
    if (cb) {
        g_openUrl.user = user;
        g_openUrl.cb = cb;
        Noesis::GUI::SetOpenUrlCallback(&g_openUrl, OpenUrlTramp);
    } else {
        g_openUrl.user = nullptr;
        g_openUrl.cb = nullptr;
        Noesis::GUI::SetOpenUrlCallback(nullptr, nullptr);
    }
}

extern "C" void noesis_open_url(const char* url) {
    Noesis::GUI::OpenUrl(url ? url : "");
}

// ── Play audio ───────────────────────────────────────────────────────────────

extern "C" void noesis_set_play_audio_callback(void* user, noesis_play_audio_cb cb) {
    if (cb) {
        g_playAudio.user = user;
        g_playAudio.cb = cb;
        Noesis::GUI::SetPlayAudioCallback(&g_playAudio, PlayAudioTramp);
    } else {
        g_playAudio.user = nullptr;
        g_playAudio.cb = nullptr;
        Noesis::GUI::SetPlayAudioCallback(nullptr, nullptr);
    }
}

extern "C" void noesis_play_audio(const char* uri, float volume) {
    Noesis::Uri u{uri ? uri : ""};
    Noesis::GUI::PlayAudio(u, volume);
}

// ── Culture ──────────────────────────────────────────────────────────────────

extern "C" void noesis_set_culture(const char* name) {
    // CultureInfo stores `name` as a raw `const char*`; SetCulture copies the
    // struct by value (and thus the pointer). Keep the string alive for the
    // process lifetime in a static buffer so the pointer stays valid for any
    // later GetCulture()/formatting use.
    static std::string sCultureName;
    sCultureName = name ? name : "";
    Noesis::CultureInfo culture;        // numberFormat keeps its literal defaults
    culture.name = sCultureName.c_str();
    Noesis::GUI::SetCulture(culture);
}

extern "C" const char* noesis_get_culture(void) {
    return Noesis::GUI::GetCulture().name;
}

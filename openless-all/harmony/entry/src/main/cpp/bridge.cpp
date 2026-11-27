#include <napi/native_api.h>
#include <string>
#include <vector>
#include <cstdint>
#include "rime_engine.h"
extern "C" char* openless_init(const char*, const char*);
extern "C" char* openless_invoke(const char*, const char*);
extern "C" void openless_free(char*);
extern "C" char* openless_feed_pcm(const char*, const uint8_t*, size_t);
static bool Read(napi_env env, napi_value value, std::string& out) {
    size_t n = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &n) != napi_ok) return false;
    std::vector<char> buf(n + 1);
    if (napi_get_value_string_utf8(env, value, buf.data(), buf.size(), &n) != napi_ok) return false;
    out.assign(buf.data(), n); return true;
}
static napi_value Call(napi_env env, napi_callback_info info, bool init) {
    size_t argc = 2; napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    std::string first, second;
    if (argc != 2 || !Read(env, args[0], first) || !Read(env, args[1], second)) {
        napi_throw_type_error(env, nullptr, "Expected two string arguments"); return nullptr;
    }
    char* response = init ? openless_init(first.c_str(), second.c_str()) : openless_invoke(first.c_str(), second.c_str());
    napi_value result; napi_create_string_utf8(env, response, NAPI_AUTO_LENGTH, &result);
    openless_free(response); return result;
}
static napi_value InitBackend(napi_env env, napi_callback_info info) { return Call(env, info, true); }
static napi_value Invoke(napi_env env, napi_callback_info info) { return Call(env, info, false); }
static napi_value InvokeAsync(napi_env env, napi_callback_info info);
static napi_value RimeAsync(napi_env env, napi_callback_info info);
static napi_value FeedPcm(napi_env env, napi_callback_info info);
static napi_value Register(napi_env env, napi_value exports) {
    napi_property_descriptor props[] = {
        {"initBackend", nullptr, InitBackend, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"invoke", nullptr, Invoke, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"invokeAsync", nullptr, InvokeAsync, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"rimeAsync", nullptr, RimeAsync, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"feedPcm", nullptr, FeedPcm, nullptr, nullptr, nullptr, napi_default, nullptr}
    };
    napi_define_properties(env, exports, 5, props); return exports;
}
static napi_module module = {1, 0, nullptr, Register, "openless_harmony", nullptr, {0}};
extern "C" __attribute__((constructor)) void RegisterOpenLess() { napi_module_register(&module); }

struct AsyncCall {
    std::string command, args, third;
    bool rime = false;
    bool audio = false;
    std::vector<uint8_t> pcm;
    char* response = nullptr;
    napi_deferred deferred;
    napi_async_work work;
};
static void ExecuteAsync(napi_env env, void* data) {
    auto call = static_cast<AsyncCall*>(data);
    call->response = call->audio ? openless_feed_pcm(call->command.c_str(), call->pcm.data(), call->pcm.size())
        : call->rime ? openless_rime_request(call->command.c_str(), call->args.c_str(), call->third.c_str())
        : openless_invoke(call->command.c_str(), call->args.c_str());
}
static void CompleteAsync(napi_env env, napi_status status, void* data) {
    auto call = static_cast<AsyncCall*>(data);
    napi_value value;
    if (status == napi_ok && call->response) {
        napi_create_string_utf8(env, call->response, NAPI_AUTO_LENGTH, &value);
        napi_resolve_deferred(env, call->deferred, value);
    } else {
        napi_create_string_utf8(env, "native async command failed", NAPI_AUTO_LENGTH, &value);
        napi_reject_deferred(env, call->deferred, value);
    }
    if (call->rime) openless_rime_free(call->response); else openless_free(call->response);
    napi_delete_async_work(env, call->work);
    delete call;
}
static napi_value InvokeAsync(napi_env env, napi_callback_info info) {
    size_t argc = 2; napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    auto call = new AsyncCall();
    if (argc != 2 || !Read(env, args[0], call->command) || !Read(env, args[1], call->args)) {
        delete call; napi_throw_type_error(env, nullptr, "Expected two string arguments"); return nullptr;
    }
    napi_value promise, name;
    napi_create_promise(env, &call->deferred, &promise);
    napi_create_string_utf8(env, "OpenLessCommand", NAPI_AUTO_LENGTH, &name);
    napi_create_async_work(env, nullptr, name, ExecuteAsync, CompleteAsync, call, &call->work);
    napi_queue_async_work(env, call->work);
    return promise;
}
static napi_value RimeAsync(napi_env env, napi_callback_info info) {
    size_t argc = 3; napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    auto call = new AsyncCall(); call->rime = true;
    if (argc != 3 || !Read(env, args[0], call->command) || !Read(env, args[1], call->args) || !Read(env, args[2], call->third)) {
        delete call; napi_throw_type_error(env, nullptr, "Expected three string arguments"); return nullptr;
    }
    napi_value promise, name;
    napi_create_promise(env, &call->deferred, &promise);
    napi_create_string_utf8(env, "OpenLessRime", NAPI_AUTO_LENGTH, &name);
    napi_create_async_work(env, nullptr, name, ExecuteAsync, CompleteAsync, call, &call->work);
    napi_queue_async_work(env, call->work);
    return promise;
}
static napi_value FeedPcm(napi_env env, napi_callback_info info) {
    size_t argc = 2; napi_value args[2]; void* bytes = nullptr; size_t length = 0;
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    auto call = new AsyncCall(); call->audio = true;
    if (argc != 2 || !Read(env, args[0], call->command) ||
        napi_get_arraybuffer_info(env, args[1], &bytes, &length) != napi_ok || !bytes ||
        !length || length > 32768 || length % 2) {
        delete call; napi_throw_type_error(env, nullptr, "Expected session and PCM16 ArrayBuffer"); return nullptr;
    }
    auto first = static_cast<uint8_t*>(bytes);
    call->pcm.assign(first, first + length);
    napi_value promise, name;
    napi_create_promise(env, &call->deferred, &promise);
    napi_create_string_utf8(env, "OpenLessPcm", NAPI_AUTO_LENGTH, &name);
    napi_create_async_work(env, nullptr, name, ExecuteAsync, CompleteAsync, call, &call->work);
    napi_queue_async_work(env, call->work);
    return promise;
}

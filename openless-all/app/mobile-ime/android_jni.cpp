#include <jni.h>
#include "rime_engine.h"
#include <string>
#include <cstring>
static std::string read(JNIEnv* env, jstring value) {
    if (!value) return {};
    // Convert using Java UTF-8, not JNI modified UTF-8 (emoji and embedded NUL).
    auto bytes = static_cast<jbyteArray>(env->CallObjectMethod(value,
        env->GetMethodID(env->FindClass("java/lang/String"), "getBytes", "(Ljava/lang/String;)[B"), env->NewStringUTF("UTF-8")));
    if (!bytes || env->ExceptionCheck()) return {};
    auto n = env->GetArrayLength(bytes);
    std::string result(n, '\0');
    env->GetByteArrayRegion(bytes, 0, n, reinterpret_cast<jbyte*>(result.data()));
    env->DeleteLocalRef(bytes);
    return result;
}
extern "C" JNIEXPORT jstring JNICALL Java_com_openless_app_OpenLessRime_nativeRequest(
    JNIEnv* env, jobject, jstring operation, jstring session, jstring argument) {
    auto op = read(env, operation), id = read(env, session), arg = read(env, argument);
    if (env->ExceptionCheck()) return nullptr;
    auto response = openless_rime_request(op.c_str(), id.c_str(), arg.c_str());
    if (!response) return nullptr;
    auto bytes = env->NewByteArray(std::strlen(response));
    env->SetByteArrayRegion(bytes, 0, std::strlen(response), reinterpret_cast<const jbyte*>(response));
    auto result = static_cast<jstring>(env->NewObject(env->FindClass("java/lang/String"),
        env->GetMethodID(env->FindClass("java/lang/String"), "<init>", "([BLjava/lang/String;)V"), bytes, env->NewStringUTF("UTF-8")));
    env->DeleteLocalRef(bytes);
    openless_rime_free(response);
    return result;
}

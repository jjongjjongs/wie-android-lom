#define _GNU_SOURCE
#include <jni.h>
#include <android/log.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>

#define TAG "WIE-WaveHook"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, TAG, __VA_ARGS__)

static JavaVM *g_vm;
static jclass g_bridge_class;
static jmethodID g_on_wave;

static uint64_t fnv1a(const int16_t *samples, size_t count) {
    const uint8_t *bytes = (const uint8_t *)samples;
    uint64_t hash = UINT64_C(0xcbf29ce484222325);
    for (size_t i = 0; i < count * 2; ++i) {
        hash ^= bytes[i];
        hash *= UINT64_C(0x100000001b3);
    }
    return hash;
}

static uint8_t hooked_play_wave(uint8_t channel, uint32_t sample_rate,
                                const int16_t *samples, size_t sample_count) {
    (void)channel;
    JNIEnv *env = NULL;
    bool consumed = false;
    if (samples && sample_count && g_vm && g_bridge_class && g_on_wave
            && (*g_vm)->GetEnv(g_vm, (void **)&env, JNI_VERSION_1_6) == JNI_OK) {
        jshortArray pcm = (*env)->NewShortArray(env, (jsize)sample_count);
        if (pcm) {
            (*env)->SetShortArrayRegion(env, pcm, 0, (jsize)sample_count, samples);
            consumed = (*env)->CallStaticBooleanMethod(env, g_bridge_class, g_on_wave,
                    (jint)sample_rate, (jint)sample_count,
                    (jlong)fnv1a(samples, sample_count), pcm);
            (*env)->DeleteLocalRef(env, pcm);
        }
        if ((*env)->ExceptionCheck(env)) {
            (*env)->ExceptionClear(env);
            consumed = false;
        }
    }
    return consumed ? 1 : 0;
}

JNIEXPORT jboolean JNICALL
Java_com_jjongjjongs_minimobile_NativeWaveBridge_nativeInstall(JNIEnv *env, jclass clazz) {
    if (!g_bridge_class) g_bridge_class = (*env)->NewGlobalRef(env, clazz);
    if (!g_on_wave) g_on_wave = (*env)->GetStaticMethodID(env, clazz, "onWave", "(IIJ[S)Z");
    if (!g_bridge_class || !g_on_wave) {
        LOGE("Java callback lookup failed");
        return JNI_FALSE;
    }

    void *handle = dlopen("libwie_android.so", RTLD_NOW | RTLD_NOLOAD);
    if (!handle) {
        LOGE("libwie_android is not loaded yet");
        return JNI_FALSE;
    }
    typedef void (*set_wave_callback_fn)(void *callback);
    set_wave_callback_fn setter = (set_wave_callback_fn)dlsym(handle, "wie_set_wave_callback");
    if (!setter) {
        LOGE("wie_set_wave_callback symbol not found");
        return JNI_FALSE;
    }
    setter((void *)hooked_play_wave);
    LOGI("stable play_wave callback installed");
    return JNI_TRUE;
}

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void)reserved;
    g_vm = vm;
    return JNI_VERSION_1_6;
}

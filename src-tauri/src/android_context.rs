use jni::{
    objects::{GlobalRef, JClass, JObject, JValue},
    JNIEnv, JavaVM,
};
use std::sync::OnceLock;

// Keep the application Context alive for every JNI bridge used by Rust. Using
// the application Context also makes this independent of Activity recreation.
static APPLICATION_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();

#[cfg(target_os = "android")]
fn initialize_tls_platform_verifier(env: &JNIEnv<'_>, context: &JObject<'_>) -> Result<(), String> {
    let raw_env = env.get_raw().cast::<jni_0_22::sys::JNIEnv>();
    let raw_context = context.as_raw().cast::<jni_0_22::sys::_jobject>();
    // SAFETY: both raw handles belong to the JNI native-method frame that is
    // active for this call. The 0.22 wrapper is only used inside that frame.
    let mut verifier_env = unsafe { jni_0_22::EnvUnowned::from_raw(raw_env) };
    let outcome = verifier_env
        .with_env(|env| -> jni_0_22::errors::Result<()> {
            // SAFETY: `raw_context` is a valid local reference for this JNI
            // frame and does not escape the closure.
            let context = unsafe { jni_0_22::objects::JObject::from_raw(env, raw_context) };
            rustls_platform_verifier::android::init_with_env(env, context)
        })
        .into_outcome();

    match outcome {
        jni_0_22::Outcome::Ok(()) => Ok(()),
        jni_0_22::Outcome::Err(error) => Err(format!(
            "Androidの証明書確認機能を初期化できませんでした: {error}"
        )),
        jni_0_22::Outcome::Panic(_) => {
            Err("Androidの証明書確認機能の初期化中に問題が発生しました。".to_string())
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_jp_mutsuna_echo_MainActivity_initializeAndroidContext(
    mut env: JNIEnv<'_>,
    _activity: JObject<'_>,
    context: JObject<'_>,
) {
    if APPLICATION_CONTEXT.get().is_some() {
        return;
    }

    #[cfg(target_os = "android")]
    {
        if let Err(error) = initialize_tls_platform_verifier(&env, &context) {
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            return;
        }
        eprintln!("[android] TLS platform verifier initialized");
    }

    let java_vm = match env.get_java_vm() {
        Ok(java_vm) => java_vm,
        Err(error) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                format!("Java VMの取得に失敗しました: {error}"),
            );
            return;
        }
    };
    let global_context = match env.new_global_ref(context) {
        Ok(context) => context,
        Err(error) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                format!("Android Contextの保持に失敗しました: {error}"),
            );
            return;
        }
    };

    if APPLICATION_CONTEXT.set(global_context).is_err() {
        return;
    }

    let context = APPLICATION_CONTEXT
        .get()
        .expect("application context was set immediately above");
    // SAFETY: both pointers are owned for the lifetime of this Android process.
    // The Java VM outlives native code and `APPLICATION_CONTEXT` holds a global
    // reference to the application Context. OnceLock guarantees one-time setup.
    unsafe {
        ndk_context::initialize_android_context(
            java_vm.get_java_vm_pointer().cast(),
            context.as_obj().as_raw().cast(),
        );
    }
}

pub(crate) fn with_bridge_env<T>(
    bridge_name: &str,
    connection_error: &str,
    call: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>, &JClass<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let context = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("{connection_error}: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("{connection_error}: {error}"))?;
    let app = unsafe { JObject::from_raw(context.context().cast()) };

    // JNI FindClass uses the bootstrap class loader on Rust worker threads.
    // Resolve app classes explicitly through the application ClassLoader.
    let class_loader = env
        .call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|value| value.l())
        .map_err(|error| format!("Androidのクラスローダーを取得できませんでした: {error}"))?;
    let class_name = env
        .new_string(bridge_name)
        .map_err(|error| format!("Androidブリッジ名を準備できませんでした: {error}"))?;
    let class_name = JObject::from(class_name);
    let class = match env
        .call_method(
            class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )
        .and_then(|value| value.l())
    {
        Ok(class) => JClass::from(class),
        Err(error) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            return Err(format!(
                "Androidブリッジ {bridge_name} を読み込めませんでした: {error}"
            ));
        }
    };

    call(&mut env, &app, &class)
}

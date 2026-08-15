# Loaded reflectively by LocalAiFeatureBridge after Play Feature Delivery installs
# this module. Keep only the native-runtime entry point required by that bridge.
-keep public class jp.mutsuna.echo.localai.LocalAiRuntimeEntry {
    public static void load(android.content.Context);
}

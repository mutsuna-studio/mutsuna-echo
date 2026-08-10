# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Rust resolves these Android bridges by their exact JNI class and method names.
# R8 cannot see those string-based calls and would otherwise remove or rename
# the methods in release builds, causing startup and recording JNI failures.
-keep class jp.mutsuna.echo.SecureCredentialBridge { *; }
-keep class jp.mutsuna.echo.RecordingBridge { *; }
-keep class jp.mutsuna.echo.AudioPlaybackBridge { *; }
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

import groovy.json.JsonSlurper

plugins {
    id("com.android.dynamic-feature")
    id("org.jetbrains.kotlin.android")
}

fun Project.findRustlsVerifierRepository(): File {
    val cargoRoot = File(projectDir, "../../..").canonicalFile
    val metadata = providers.exec {
        workingDir = cargoRoot
        commandLine("cargo", "metadata", "--format-version", "1", "--filter-platform", "aarch64-linux-android", "--manifest-path", "Cargo.toml")
    }.standardOutput.asText.get()
    val packages = (JsonSlurper().parseText(metadata) as Map<*, *>)["packages"] as List<*>
    val manifest = packages.asSequence().map { it as Map<*, *> }
        .first { it["name"] == "rustls-platform-verifier-android" }["manifest_path"] as String
    return File(File(manifest).parentFile, "maven")
}

repositories {
    maven {
        url = uri(project.findRustlsVerifierRepository())
        metadataSources.artifact()
    }
}

android {
    namespace = "jp.mutsuna.echo.localai"
    compileSdk = 36
    defaultConfig { minSdk = 29 }
    buildTypes {
        getByName("release") {
            proguardFiles("proguard-rules.pro")
        }
    }
    flavorDimensions += "abi"
    productFlavors {
        create("universal") { dimension = "abi" }
        create("arm64") { dimension = "abi" }
        create("arm") { dimension = "abi" }
        create("x86") { dimension = "abi" }
        create("x86_64") { dimension = "abi" }
    }
    kotlinOptions { jvmTarget = "1.8" }
}

dependencies {
    implementation(project(":app"))
    implementation("com.google.android.play:feature-delivery:2.1.0")
}

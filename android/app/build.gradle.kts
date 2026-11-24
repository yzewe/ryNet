import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.ktlint)
}

val signing: Properties? = run {
    val env = System.getenv()
    env["RYNET_KEYSTORE"]?.let { store ->
        return@run Properties().apply {
            setProperty("storeFile", store)
            setProperty("storePassword", env["RYNET_KEYSTORE_PASSWORD"].orEmpty())
            setProperty("keyAlias", env["RYNET_KEY_ALIAS"].orEmpty())
            setProperty("keyPassword", env["RYNET_KEY_PASSWORD"] ?: env["RYNET_KEYSTORE_PASSWORD"].orEmpty())
        }
    }
    val f = file(env["RYNET_SIGNING"] ?: "${System.getProperty("user.home")}/.rynet-signing/keystore.properties")
    if (f.isFile) Properties().apply { f.inputStream().use { load(it) } } else null
}

val tv = hasProperty("tv")
val legacy = hasProperty("legacy") || tv
val play = hasProperty("play")
val coreAbi = (findProperty("abi") as String?) ?: if (legacy || play) "armeabi-v7a,arm64-v8a" else "arm64-v8a"
val coreApi = if (legacy) 24 else 26

android {
    namespace = "dev.yzewe.rynet"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "dev.yzewe.rynet"
        minSdk = coreApi
        targetSdk = 36
        versionCode = 215
        versionName = "1.20.5"
        ndk { abiFilters += coreAbi.split(",") }
        manifestPlaceholders["tvLauncher"] = tv.toString()
        resValue("bool", "play", play.toString())
    }

    signingConfigs {
        if (signing != null) {
            create("release") {
                storeFile = file(signing.getProperty("storeFile"))
                storePassword = signing.getProperty("storePassword")
                keyAlias = signing.getProperty("keyAlias")
                keyPassword = signing.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        isCoreLibraryDesugaringEnabled = legacy
    }

    if (legacy) sourceSets["main"].res.srcDir("src/legacy/res")
    if (play) sourceSets["release"].manifest.srcFile("src/play/AndroidManifest.xml")
    if (tv) sourceSets["release"].manifest.srcFile("src/tv/AndroidManifest.xml")

    buildFeatures {
        compose = true
        buildConfig = false
        resValues = true
    }

    androidResources { localeFilters += listOf("en", "ru") }

    packaging {
        jniLibs { useLegacyPackaging = true }
        resources.excludes += listOf(
            "META-INF/**/LICENSE*",
            "META-INF/*.version",
            "META-INF/**/*.kotlin_module",
            "kotlin/**",
            "DebugProbesKt.bin",
            "META-INF/com/android/build/gradle/*",
        )
    }

    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
        unitTests.all { it.jvmArgs("--add-exports=java.base/jdk.internal.access=ALL-UNNAMED") }
    }
}

val buildCore by tasks.registering(Exec::class) {
    group = "build"
    description = "Build the Rust core for $coreAbi"
    val core = rootProject.projectDir.resolve("../core")
    workingDir = core
    commandLine("bash", core.resolve("build-android.sh").absolutePath, projectDir.resolve("src/main/jniLibs").absolutePath, coreAbi)
    environment("API", coreApi.toString())
    inputs.property("abi", coreAbi)
    inputs.property("api", coreApi)
    inputs.dir(core).withPropertyName("coreSources").optional()
    outputs.dir(projectDir.resolve("src/main/jniLibs"))
}
tasks.named("preBuild") { dependsOn(buildCore) }

dependencies {
    if (legacy) coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.material3)
    implementation(libs.activity.compose)
    implementation(libs.androidx.core)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.compose.icons.core)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.zxing.core)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.robolectric)
    testImplementation(libs.junit)
}

ktlint {
    version.set(libs.versions.ktlint.get())
    android.set(true)
}

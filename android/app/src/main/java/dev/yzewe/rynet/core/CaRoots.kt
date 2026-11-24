package dev.yzewe.rynet.core

object CaRoots {
    @Volatile private var done = false

    fun ensure() {
        if (done) return
        synchronized(this) {
            if (done) return
            val certs = runCatching {
                val ks = java.security.KeyStore.getInstance("AndroidCAStore").apply { load(null) }
                ks.aliases().toList().filter { it.startsWith("system:") }.mapNotNull { ks.getCertificate(it)?.encoded }
            }.getOrDefault(emptyList())
            val accepted = if (certs.isNotEmpty()) Native.setRoots(certs.toTypedArray()) else 0
            android.util.Log.i("rynet", "CA roots: $accepted of ${certs.size}")
            if (accepted > 0) done = true
        }
    }
}

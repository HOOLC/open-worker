package org.rustls.platformverifier

// Owned by the synchronized verifier. This does not cache successful trust
// decisions. Memory and failure lifetime stay bounded even across many peers.
internal class VerificationFailures<K, V>(
    private val now: () -> Long = { System.nanoTime() },
    private val lifetimeNanos: Long = 30_000_000_000L,
    private val capacity: Int = 64,
) {
    private data class Entry<V>(val value: V, val recorded: Long)
    private val entries = LinkedHashMap<K, Entry<V>>()

    fun clear() { entries.clear() }

    fun get(key: K): V? {
        val entry = entries[key] ?: return null
        if (now() - entry.recorded >= lifetimeNanos) {
            entries.remove(key)
            return null
        }
        return entry.value
    }

    fun put(key: K, value: V) {
        entries.remove(key)
        if (entries.size >= capacity) entries.remove(entries.keys.first())
        entries[key] = Entry(value, now())
    }
}

package dev.yzewe.rynet.ui

import dev.yzewe.rynet.data.Group
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.Section
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.data.Subscription
import org.junit.Assert.assertEquals
import org.junit.Test

class ServerSearchTest {
    private fun p(name: String, kind: String = "REALITY", description: String? = null, placeholder: Boolean = false) = Profile(name, name, "", Group.Main, kind, true, emptyList(), description, placeholder)

    private val list = listOf(
        p("━━ ОСНОВНЫЕ ━━", placeholder = true),
        p("Германия"),
        p("Нидерланды", kind = "Hysteria 2"),
        p("━━ Wi-Fi локации ━━", placeholder = true),
        p("Германия Wi-Fi", description = "Только мобильная сеть"),
        p("Подписка истекла", placeholder = true)
    )
    private val sections = Subscription.sections(list)

    private fun names(q: String) = search(sections, q).map { s -> s.title to s.items.map { it.name } }

    @Test
    fun emptyQueryKeepsEverything() {
        assertEquals(sections, search(sections, "  "))
    }

    @Test
    fun sectionsStayAsWithoutSearch() {
        assertEquals(listOf("ОСНОВНЫЕ" to listOf("Германия"), "Wi-Fi локации" to listOf("Германия Wi-Fi")), names("герм"))
    }

    @Test
    fun everyWordMustMatch() {
        assertEquals(listOf("Wi-Fi локации" to listOf("Германия Wi-Fi")), names("германия wi-fi"))
        assertEquals(listOf("ОСНОВНЫЕ" to listOf("Нидерланды")), names("hysteria"))
    }

    @Test
    fun sectionTitleAndDescriptionMatch() {
        assertEquals(listOf("Wi-Fi локации" to listOf("Германия Wi-Fi")), names("локации"))
        assertEquals(listOf("Wi-Fi локации" to listOf("Германия Wi-Fi")), names("мобильная"))
    }

    @Test
    fun yoIsE() {
        assertEquals(listOf("Wi-Fi локации" to listOf("Германия Wi-Fi")), names("мобильнаЯ сЕть"))
        assertEquals(names("сеть"), names("сёть"))
    }

    @Test
    fun providerMessagesAndMissesHidden() {
        assertEquals(emptyList<Pair<String, List<String>>>(), names("истекла"))
        assertEquals(emptyList<Pair<String, List<String>>>(), names("япония"))
    }

    @Test
    fun balancersStayFirstWhenSorted() {
        val auto = p("Автовыбор").copy(members = listOf(p("Германия"), p("Нидерланды")))
        val all = listOf(Section("", listOf(p("Швеция"), p("Германия"), auto)))
        val pings = mapOf("Швеция" to 40, "Германия" to 90, "Автовыбор" to 120)
        assertEquals(listOf("Автовыбор", "Швеция", "Германия"), sorted(all, ServerSort.Ping, pings).single().items.map { it.name })
        assertEquals(listOf("Автовыбор", "Германия", "Швеция"), sorted(all, ServerSort.Name, pings).single().items.map { it.name })
    }
}

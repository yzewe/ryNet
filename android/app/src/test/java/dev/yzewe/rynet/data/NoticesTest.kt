package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class NoticesTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Test fun dismissedNoticeStaysHiddenAfterRestartButChangedTextShowsAgain() {
        val key = Settings.noticeKey("sub1", "Скидка до пятницы")
        Settings.save(ctx, Settings().dismissNotice(key))
        val saved = Settings.load(ctx)
        assertTrue(saved.noticeHidden(key))
        assertFalse(saved.noticeHidden(Settings.noticeKey("sub1", "Скидка до воскресенья")))
        assertFalse(saved.noticeHidden(Settings.noticeKey("sub2", "Скидка до пятницы")))
    }

    @Test fun dismissingTwiceKeepsOneKeyAndOldOnesAreEvicted() {
        val once = Settings().dismissNotice("a:1")
        assertSame(once, once.dismissNotice("a:1"))
        var s = Settings()
        for (i in 0 until 250) s = s.dismissNotice("k:$i")
        assertEquals(200, s.dismissedNotices.size)
        assertFalse(s.noticeHidden("k:0"))
        assertTrue(s.noticeHidden("k:249"))
    }
}

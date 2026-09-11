package surf.zork.android

import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.view.FrameMetrics
import androidx.lifecycle.lifecycleScope
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.launch
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class ConversationScrollTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private fun launch(empty: Boolean = false) = ActivityScenario.launch<ConversationScrollActivity>(Intent(instrumentation.targetContext,ConversationScrollActivity::class.java).putExtra("empty",empty))
    private fun settled() { instrumentation.waitForIdleSync(); Thread.sleep(150); instrumentation.waitForIdleSync() }
    private fun move(scenario: ActivityScenario<ConversationScrollActivity>, index: Int, animate: Boolean = false) {
        val done=CountDownLatch(1)
        scenario.onActivity { a -> a.lifecycleScope.launch(androidx.compose.ui.platform.AndroidUiDispatcher.Main) { try { if(animate)a.scroll.animateScrollToItem(index) else a.scroll.scrollToItem(index) } finally {done.countDown()} } }
        assertTrue(done.await(10,TimeUnit.SECONDS)); settled()
    }
    @Test fun initialEntryAndNewMessagesFollowOnlyAtBottom() {
        launch(empty=true).use { scenario ->
            settled(); scenario.onActivity { it.load(80) }; settled()
            scenario.onActivity { assertFalse("First nonempty load must end at bottom",it.scroll.canScrollForward) }
            scenario.onActivity { it.append() }; settled()
            scenario.onActivity { assertFalse("Append at bottom must stay at bottom",it.scroll.canScrollForward) }
            scenario.onActivity { it.growTail() }; settled()
            scenario.onActivity { assertFalse("A tall final message must also reach its end",it.scroll.canScrollForward) }
            move(scenario,20,true)
            var index=0;var offset=0
            scenario.onActivity { index=it.scroll.firstVisibleItemIndex;offset=it.scroll.firstVisibleItemScrollOffset;assertTrue(it.scroll.canScrollForward);it.append() };settled()
            scenario.onActivity { assertEquals(index,it.scroll.firstVisibleItemIndex);assertEquals(offset,it.scroll.firstVisibleItemScrollOffset) }
            move(scenario,83,true)
            scenario.onActivity { assertFalse(it.scroll.canScrollForward);it.append() };settled()
            scenario.onActivity { assertFalse("Returning to bottom enables following again",it.scroll.canScrollForward) }
        }
    }
    @Test fun entryDoesNotPaintTheTopBeforeTheBottom() {
        launch(empty=true).use { scenario ->
            settled();scenario.onActivity { it.drawnPositions.clear();it.load(80) };settled()
            scenario.onActivity {
                val frames = it.drawnPositions.toList()
                instrumentation.targetContext.filesDir.resolve("conversation-entry-frames.json").writeText(org.json.JSONArray(frames.map { f -> JSONObject().put("first",f.first).put("can_scroll_forward",f.canScrollForward) }).toString())
                assertTrue("No message draw recorded",frames.isNotEmpty())
                assertTrue("Visible jump during initial draw: $frames",frames.none { f -> f.canScrollForward })
            }
        }
    }
    @Test fun headerAvatarKeepsItsPositionWhenParticipantsArrive() {
        val automation = instrumentation.uiAutomation
        val intent=Intent(instrumentation.targetContext,ConversationScrollActivity::class.java).putExtra("header",true)
        ActivityScenario.launch<ConversationScrollActivity>(intent).use { scenario ->
            settled()
            fun avatarBounds():android.graphics.Rect {
                fun find(node:android.view.accessibility.AccessibilityNodeInfo?):android.view.accessibility.AccessibilityNodeInfo? {
                    if(node==null)return null
                    if(node.contentDescription?.toString()=="滚动验证")return node
                    for(i in 0 until node.childCount)find(node.getChild(i))?.let{return it}
                    return null
                }
                var node=find(automation.rootInActiveWindow)
                val deadline=android.os.SystemClock.uptimeMillis()+3000
                while(node==null && android.os.SystemClock.uptimeMillis()<deadline) {
                    Thread.sleep(100)
                    node=find(automation.rootInActiveWindow)
                }
                assertNotNull("Header avatar missing",node)
                return android.graphics.Rect().also { node!!.getBoundsInScreen(it) }
            }
            val before=avatarBounds();scenario.onActivity { it.completeMembers() };settled()
            assertEquals("Loading participants must not move the header avatar",before,avatarBounds())
        }
    }
    @Test fun loadingIndicatorIsReplacedByLoadedHistory() {
        val automation = instrumentation.uiAutomation
        val intent = Intent(instrumentation.targetContext, ConversationScrollActivity::class.java)
            .putExtra("empty", true).putExtra("loading", true)
        ActivityScenario.launch<ConversationScrollActivity>(intent).use { scenario ->
            fun loadingVisible(): Boolean {
                fun find(node: android.view.accessibility.AccessibilityNodeInfo?): Boolean {
                    if (node == null) return false
                    if (node.text?.toString()?.contains("正在加载消息") == true) return true
                    return (0 until node.childCount).any { find(node.getChild(it)) }
                }
                return find(automation.rootInActiveWindow)
            }
            val deadline = android.os.SystemClock.uptimeMillis() + 3000
            while (!loadingVisible() && android.os.SystemClock.uptimeMillis() < deadline) Thread.sleep(50)
            assertTrue("Loading must appear in the empty message area", loadingVisible())
            automation.takeScreenshot()?.let { screenshot ->
                instrumentation.targetContext.filesDir.resolve("conversation-loading.png").outputStream().use {
                    screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)
                }
                screenshot.recycle()
            }
            scenario.onActivity { it.load(80) }; settled()
            assertFalse("Loaded history must replace the loading indicator", loadingVisible())
            scenario.onActivity { assertFalse(it.scroll.canScrollForward) }
        }
    }
    @Test fun measureScrollFrames() {
        launch().use { scenario ->
            settled()
            var cold=emptyList<Double>()
            scenario.onActivity { cold=synchronized(it.initialFrames){it.initialFrames.sorted()} }
            val samples=mutableListOf<Double>()
            scenario.onActivity { a -> a.window.addOnFrameMetricsAvailableListener({_,metrics,_-> synchronized(samples) { samples.add((metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION)+metrics.getMetric(FrameMetrics.DRAW_DURATION))/1_000_000.0) } },Handler(Looper.getMainLooper())) }
            repeat(4) { move(scenario,0,true);move(scenario,80,true) }
            val sorted=synchronized(samples){samples.sorted()};assertTrue("Frame samples missing",sorted.size>=10)
            fun percentile(p:Double)=sorted[((sorted.size-1)*p).toInt()]
            val result=JSONObject().put("cold_layout_draw_p95_ms",cold.getOrNull(((cold.size-1)*.95).toInt())).put("cold_layout_draw_p99_ms",cold.getOrNull(((cold.size-1)*.99).toInt())).put("frames",sorted.size).put("layout_draw_p95_ms",percentile(.95)).put("layout_draw_p99_ms",percentile(.99)).put("messages",80).put("width",390).put("height",844)
            instrumentation.targetContext.filesDir.resolve("conversation-scroll-performance.json").writeText(result.toString())
        }
    }
}

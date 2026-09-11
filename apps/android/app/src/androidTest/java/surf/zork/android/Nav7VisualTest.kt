package surf.zork.android

import android.content.Intent
import android.graphics.Bitmap
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class Nav7VisualTest {
    @Test fun failedMessageOffersManualResendAndDelete() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val intent = Intent(instrumentation.targetContext, Nav7PreviewActivity::class.java).putExtra("screen", "delivery")
        ActivityScenario.launch<Nav7PreviewActivity>(intent).use { scenario ->
            instrumentation.waitForIdleSync()
            val until = android.os.SystemClock.uptimeMillis() + 5000
            while (instrumentation.uiAutomation.rootInActiveWindow?.findAccessibilityNodeInfosByText("发送失败").isNullOrEmpty() && android.os.SystemClock.uptimeMillis() < until) Thread.sleep(50)
            for (label in listOf("发送失败", "发送中", "重发", "删除"))
                assertFalse("Missing $label", instrumentation.uiAutomation.rootInActiveWindow.findAccessibilityNodeInfosByText(label).isEmpty())
            for ((x, action) in listOf(284f to "resend:failed", 342f to "delete:failed")) {
                val bounds = android.graphics.Rect()
                scenario.onActivity { bounds.set(it.contentBounds) }
                val down = android.os.SystemClock.uptimeMillis()
                for (actionCode in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
                    val event = android.view.MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), actionCode, bounds.left + x, bounds.top + 177f, 0)
                    instrumentation.sendPointerSync(event); event.recycle()
                }
                instrumentation.waitForIdleSync()
                scenario.onActivity { assertEquals(action,it.lastAction) }
            }
        }
    }

    @Test fun captureProductionComponents() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val folder = File(context.filesDir, "nav7").apply { mkdirs() }
        for (screen in listOf("navigation", "conversation", "composer", "device", "delivery")) {
            val intent = Intent(context, Nav7PreviewActivity::class.java).putExtra("screen", screen)
            ActivityScenario.launch<Nav7PreviewActivity>(intent).use { scenario ->
                val frames = CountDownLatch(1)
                scenario.onActivity { activity -> activity.window.decorView.postOnAnimation {
                    activity.window.decorView.postOnAnimation { frames.countDown() }
                } }
                assertTrue(frames.await(10, TimeUnit.SECONDS))
                instrumentation.waitForIdleSync()
                var bounds = android.graphics.Rect()
                scenario.onActivity { bounds = android.graphics.Rect(it.contentBounds) }
                assertEquals(390, bounds.width()); assertEquals(844, bounds.height())
                val crop = Bitmap.createBitmap(390, 844, Bitmap.Config.ARGB_8888)
                val copied = CountDownLatch(1); var result = -1
                scenario.onActivity { activity -> android.view.PixelCopy.request(activity.window, bounds, crop, {
                    result = it; copied.countDown()
                }, android.os.Handler(android.os.Looper.getMainLooper())) }
                assertTrue(copied.await(5, TimeUnit.SECONDS)); assertEquals(android.view.PixelCopy.SUCCESS, result)
                File(folder, "$screen-390.png").outputStream().use { crop.compress(Bitmap.CompressFormat.PNG, 100, it) }
                if(screen in listOf("conversation","composer")) {
                    fun sendNode(node:android.view.accessibility.AccessibilityNodeInfo?):android.view.accessibility.AccessibilityNodeInfo? {
                        if(node==null)return null;if(node.contentDescription?.toString()=="发送")return node
                        for(i in 0 until node.childCount)sendNode(node.getChild(i))?.let{return it};return null
                    }
                    val send=sendNode(instrumentation.uiAutomation.rootInActiveWindow)
                    assertNotNull("Send action exists",send)
                    val hit=android.graphics.Rect();send!!.getBoundsInScreen(hit)
                    assertTrue("Send hit target remains touch sized",hit.width()>=44 && hit.height()>=44)
                }
                if (screen == "navigation") {
                    val x = bounds.left + 352f; val y = bounds.top + 154f
                    val down = android.os.SystemClock.uptimeMillis()
                    fun pointer(action: Int, px: Float = x, py: Float = y) {
                        val event = android.view.MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), action, px, py, 0)
                        instrumentation.sendPointerSync(event); event.recycle()
                    }
                    pointer(android.view.MotionEvent.ACTION_DOWN)
                    android.os.SystemClock.sleep(180)
                    val pressed = Bitmap.createBitmap(390, 844, Bitmap.Config.ARGB_8888); val ready = CountDownLatch(1)
                    scenario.onActivity { android.view.PixelCopy.request(it.window, bounds, pressed, { ready.countDown() }, android.os.Handler(android.os.Looper.getMainLooper())) }
                    assertTrue(ready.await(5, TimeUnit.SECONDS))
                    assertEquals("Leader press feedback covers the full row", pressed.getPixel(12,154), pressed.getPixel(378,154))
                    assertNotEquals("Press feedback must be visible", crop.getPixel(12,154), pressed.getPixel(12,154))
                    File(folder,"navigation-pressed-390.png").outputStream().use { pressed.compress(Bitmap.CompressFormat.PNG,100,it) }; pressed.recycle()
                    pointer(android.view.MotionEvent.ACTION_UP)
                    instrumentation.waitForIdleSync()
                    scenario.onActivity { assertEquals("Right edge belongs to the same leader row", "leader:product", it.lastAction) }
                }
                crop.recycle()
            }
        }
    }
}

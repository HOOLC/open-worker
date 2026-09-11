package surf.zork.android

import android.content.Intent
import android.graphics.Bitmap
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class ClientSettingsTest {
    @Test fun diagnosticsAreRedactedAndAppearanceIsAvailable() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val folder = File(context.filesDir, "client-settings").apply { mkdirs() }
        fun find(text: String): AccessibilityNodeInfo? {
            instrumentation.uiAutomation.clearCache()
            fun walk(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
                if (node == null) return null
                if (node.text?.toString() == text || node.text?.toString()?.lineSequence()?.any { it == text } == true || node.contentDescription?.toString() == text) return node
                for (i in 0 until node.childCount) walk(node.getChild(i))?.let { return it }
                return null
            }
            return walk(instrumentation.uiAutomation.rootInActiveWindow)
        }
        fun click(text: String) {
            var node = find(text)
            val until = System.currentTimeMillis() + 5000
            while (node == null && System.currentTimeMillis() < until) { Thread.sleep(80); node = find(text) }
            assertNotNull(text, node)
            while (node != null && !node.isClickable) node = node.parent
            assertTrue(text, node!!.performAction(AccessibilityNodeInfo.ACTION_CLICK))
            instrumentation.waitForIdleSync(); Thread.sleep(300)
        }
        fun capture(name: String) {
            val image = instrumentation.uiAutomation.takeScreenshot()
            assertNotNull(image)
            File(folder, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
            image.recycle()
        }
        run {
            ActivityScenario.launch<Nav7PreviewActivity>(Intent(context, Nav7PreviewActivity::class.java).putExtra("screen", "home").putExtra("width", 0)).use { scenario ->
                instrumentation.waitForIdleSync(); Thread.sleep(600)
                assertNull(find("这台手机")); assertNull(find("查看连接身份")); assertNull(find("通知"))
                capture("home")
                assertNotNull(find("外观")); assertNull(find("文字大小"))
                scenario.recreate(); instrumentation.waitForIdleSync(); Thread.sleep(500)
                assertNotNull(find("外观"))
                click("帮助与诊断"); click("检查连接")
                Thread.sleep(700); capture("diagnostics")
                assertNotNull("reachable result", find("可连接")); assertNotNull("unreachable result", find("未连通"))
                click("复制诊断信息"); assertNotNull(find("已复制诊断信息"))
                click("返回设置"); click("关于 Zork"); capture("about")
                click("Zork 开源许可"); assertNotNull(find("MIT License"))
            }
            val report = ConnectionDiagnosis("now", "Wi-Fi", listOf(DeviceDiagnosis("secret-device-192.168.1.2", false))).report("0.1")
            assertFalse(report.contains("secret-device")); assertFalse(report.contains("192.168")); assertTrue(report.contains("设备 1：未连通"))
        }
    }
}

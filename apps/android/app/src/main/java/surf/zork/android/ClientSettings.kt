package surf.zork.android

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Date

internal data class DeviceDiagnosis(val name: String, val reachable: Boolean)
internal data class ConnectionDiagnosis(val time: String, val network: String, val devices: List<DeviceDiagnosis>) {
    // An allowlist, deliberately excluding peer names, addresses, identity and server error text.
    fun report(version: String): String = buildString {
        appendLine("Zork Android $version")
        appendLine("Android ${Build.VERSION.RELEASE}")
        appendLine("检查时间：$time")
        appendLine("网络：$network")
        appendLine("设备数量：${devices.size}")
        devices.forEachIndexed { index, device -> appendLine("设备 ${index + 1}：${if (device.reachable) "可连接" else "未连通"}") }
    }
}

@Suppress("DEPRECATION")
internal fun clientVersion(context: Context): String = context.packageManager.getPackageInfo(context.packageName, 0).let {
    "${it.versionName} (${it.longVersionCode})"
}

private fun networkDescription(context: Context): String {
    val manager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    val network = manager.getNetworkCapabilities(manager.activeNetwork) ?: return "无可用网络"
    return when {
        network.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> "VPN"
        network.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "Wi-Fi"
        network.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "移动网络"
        network.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "有线网络"
        else -> "其他网络"
    }
}

@Composable
internal fun ClientSettingsScreen(page: String, back: () -> Unit,
    diagnose: suspend () -> List<DeviceDiagnosis>, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var checking by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<ConnectionDiagnosis?>(null) }
    var failed by remember { mutableStateOf(false) }
    var copied by remember { mutableStateOf(false) }
    var showLicense by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val version = remember(context) { clientVersion(context) }
    val title = when (page) { "diagnostics" -> "帮助与诊断"; else -> "关于 Zork" }
    Column(modifier.fillMaxSize().background(ZorkColors.Canvas)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = back, modifier = Modifier.size(44.dp)) {
                Icon(painterResource(R.drawable.ic_arrow_left), "返回设置", Modifier.size(22.dp))
            }
            Text(title, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
        }
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
            when (page) {
                "diagnostics" -> {
                    Text("连接检查", fontSize = 16.sp, fontWeight = FontWeight.Medium)
                    Text("检查当前网络，以及已连接设备是否能响应。", fontSize = 13.sp, lineHeight = 21.sp, color = ZorkColors.Muted)
                    SettingsButton(if (checking) "正在检查…" else "检查连接", primary = true, enabled = !checking) {
                        scope.launch {
                            checking = true; copied = false; failed = false; result = null
                            try {
                                val network = networkDescription(context)
                                val devices = diagnose()
                                result = ConnectionDiagnosis(DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.MEDIUM).format(Date()), network, devices)
                            } catch (e: CancellationException) { throw e }
                            catch (_: Exception) { failed = true }
                            finally { checking = false }
                        }
                    }
                    if (failed) Text("检查未完成，请重试。", color = ZorkColors.Danger, fontSize = 14.sp)
                    result?.let { report ->
                        Text("${report.time} · ${report.network}", color = ZorkColors.Muted, fontSize = 12.sp)
                        if (report.devices.isEmpty()) Text("还没有连接设备。返回导航，选择「连接设备」开始。", fontSize = 14.sp, lineHeight = 22.sp)
                        report.devices.forEach { device ->
                            Column(Modifier.fillMaxWidth().background(ZorkColors.Paper, SettingsStyle.Card).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                Text(device.name, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                                Text(if (device.reachable) "可连接" else "未连通", fontSize = 13.sp, color = if (device.reachable) ZorkColors.Online else ZorkColors.Warning)
                                if (!device.reachable) Text("确认设备已联网、Zork 正在运行，且仍允许此客户端访问，然后重新检查。", fontSize = 13.sp, lineHeight = 21.sp, color = ZorkColors.Muted)
                            }
                        }
                        SettingsButton(if (copied) "已复制诊断信息" else "复制诊断信息") {
                            (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Zork 连接诊断", report.report(version)))
                            copied = true
                        }
                        Text("仅复制版本、网络类型和检查结果，不包含设备名称、地址、身份标识或聊天内容。", fontSize = 12.sp, lineHeight = 20.sp, color = ZorkColors.Muted)
                    }
                    Text("收不到消息时", fontSize = 16.sp, fontWeight = FontWeight.Medium)
                    Text("打开应用后会恢复连接并补齐消息。目前离开应用会暂停连接，后台不会推送通知。", fontSize = 14.sp, lineHeight = 23.sp, color = ZorkColors.Muted)
                }
                else -> {
                    Text("Zork", fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
                    Text("和你的小伙伴一起完成任务。", fontSize = 15.sp, lineHeight = 24.sp)
                    Text("客户端版本 $version", fontSize = 13.sp, color = ZorkColors.Muted)
                    SettingsButton("Zork 开源许可") { showLicense = true }
                }
            }
        }
    }
    if (showLicense) SettingsSheet("Zork 开源许可", dismiss = { showLicense = false }) {
        val license = remember(context) { context.resources.openRawResource(R.raw.zork_license).bufferedReader().use { it.readText() } }
        Text(license, fontSize = 13.sp, lineHeight = 21.sp)
    }
}

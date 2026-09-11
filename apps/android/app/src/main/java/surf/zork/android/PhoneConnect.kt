package surf.zork.android

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.CaptureActivity
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions

/** Uses the bundled decoder and Android camera; no Play Services or cloud scan. */
class QrScanActivity : CaptureActivity()

@Composable
internal fun PhoneConnectActions(model: ClientViewModel) {
    var paste by remember { mutableStateOf(false) }
    var ticket by remember { mutableStateOf("") }
    var scanNotice by remember { mutableStateOf<String?>(null) }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        val value = result.contents
        if (value != null) { scanNotice = null; model.beginInvitation(value) }
        else if (result.originalIntent?.getBooleanExtra("MISSING_CAMERA_PERMISSION", false) == true) {
            scanNotice = "未获得相机权限，也可以粘贴桌面的连接邀请。"
            paste = true
        }
    }
    Text("在电脑中选择「连接设备 → 连接手机」，然后扫描二维码。", color = ZorkColors.Muted, fontSize = 14.sp, lineHeight = 23.sp)
    Button(onClick = {
        scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setPrompt("扫描电脑 Zork 的连接二维码").setBeepEnabled(false)
            .setOrientationLocked(false).setCaptureActivity(QrScanActivity::class.java))
    }, enabled = !model.busy && model.ready, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), shape = RoundedCornerShape(12.dp)) { Text("扫一扫连接") }
    TextButton(onClick = { paste = !paste }, enabled = !model.busy) { Text("粘贴连接邀请") }
    if (paste) {
        FormField(ticket, { if (it.length <= 32768) ticket = it }, label = { Text("连接邀请") },
            modifier = Modifier.fillMaxWidth(), maxLines = 4)
        Button(onClick = { model.beginInvitation(ticket) }, enabled = ticket.isNotBlank() && !model.busy && model.ready,
            modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp)) { Text("连接") }
    }
    (scanNotice ?: model.notice)?.let { Text(it, color = ZorkColors.Danger, fontSize = 13.sp) }
}

@Composable
internal fun PhoneInvitationStatus(model: ClientViewModel) {
    val invitation = model.invitation ?: return
    Text(invitation.text("name"), fontSize = 20.sp)
    Text(if (invitation.text("status") == "awaiting_approval") "请在电脑上允许这台手机连接。" else "正在连接设备…",
        color = ZorkColors.Muted, fontSize = 14.sp, lineHeight = 23.sp)
    model.notice?.let { Text(it, color = ZorkColors.Danger, fontSize = 13.sp) }
    TextButton(onClick = model::cancelInvitation, enabled = !model.busy) { Text("取消连接") }
}

package surf.zork.android

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.animation.animateColorAsState
import androidx.compose.ui.draw.drawBehind
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.core.content.res.ResourcesCompat

class MainActivity : ComponentActivity() {
    private val model: ClientViewModel by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = androidx.activity.SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT),
            navigationBarStyle = androidx.activity.SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT),
        )
        setContent { ZorkTheme {
            val lightPage = model.conversation != null || model.settings != null
            val chrome = animateColorAsState(if (lightPage) ZorkColors.Canvas else ZorkColors.Paper,
                animationSpec = PageSlideMotion.spec(lightPage), label = "system-bar-background")
            CompositionLocalProvider(LocalMessagePreviewHeight provides model.messagePreviewHeight) {
                Box(Modifier.fillMaxSize().drawBehind { drawRect(chrome.value) }) { ClientScreen(model) }
            }
        } }
    }
    override fun onStart() { super.onStart(); model.foreground(true) }
    override fun onStop() {
        if (!isChangingConfigurations) model.foreground(false)
        super.onStop()
    }
}

@Composable
private fun ClientScreen(model: ClientViewModel) {
    var addDevice by rememberSaveable { mutableStateOf(false) }
    var hadInvitation by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(model.invitation?.text("id")) {
        if (model.invitation != null) { hadInvitation = true; addDevice = true }
        else if (hadInvitation) { hadInvitation = false; addDevice = false }
    }
    var editingComment by remember { mutableStateOf<DraftCommentUi?>(null) }
    var fullMessage by remember(model.activePeer?.id, model.conversation?.id) { mutableStateOf<ChatMessage?>(null) }
    var attachmentPeer by rememberSaveable { mutableStateOf<String?>(null) }
    var attachmentSession by rememberSaveable { mutableStateOf<String?>(null) }
    var exporting by remember { mutableStateOf<TextAttachmentUi?>(null) }
    val pickFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        val peer = attachmentPeer; val session = attachmentSession
        if (uri != null && peer != null && session != null) model.addTextAttachment(uri, peer, session)
    }
    val saveFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.CreateDocument("text/plain")) { uri ->
        val file = exporting; if (uri != null && file != null) model.exportTextAttachment(uri, file)
        exporting = null
    }
    BackHandler(enabled = model.settings != null || model.conversation != null) {
        if (fullMessage != null) fullMessage = null else if (model.settings != null) model.backSettings() else model.back()
    }
    val retained = androidx.compose.runtime.saveable.rememberSaveableStateHolder()
    val currentSettings = model.settings
    val workbenchState = WorkbenchState(model.messageRevision, model.peers, model.activePeer, model.conversation, model.leaders, model.sessions,
            model.tasksByLeader, model.messages, model.pending, model.draft, model.olderCursor != null,
            model.busy || model.loadingOlder, model.ready, model.connected, model.notice, model.activity, model.running,
            model.comments, model.participants, model.deviceTrees, model.attachments, model.historyLoading, model.conversationEntry, model.messageActivity, newer = model.hasNewer)
    val routeKey = fullMessage?.let { "message:${it.id}" } ?: settingsRouteKey(currentSettings)
    PageSlide(ClientPage(currentSettings, workbenchState, fullMessage), routeKey, if (fullMessage != null) 2 else settingsRouteDepth(currentSettings),
        if (currentSettings != null || model.conversation != null) ZorkColors.Canvas else ZorkColors.Paper,
        Modifier.safeDrawingPadding().imePadding()) { shown, active ->
    if (shown.message != null) {
        FullMessagePage(shown.message, { fullMessage = null }) { quote ->
            model.conversation?.let { conversation ->
                val row = shown.message
                editingComment = DraftCommentUi(NativeBridge.newId(), conversation.id, row.id,
                    row.author, row.authorAgentId.ifBlank { null }, quote, "")
            }
        }
    } else if (shown.settings != null) {
        retained.SaveableStateProvider(settingsRouteKey(shown.settings)) {
            MobileSettings(shown.settings, shown.workbench.peers, if (!active) SettingsActions() else SettingsActions(model::backSettings, { model.showDevice(it) },
                model::settingsPage, model::settingsProfile, model::assistSettings, model::checkUpdate,
                { addDevice = true }, refresh = model::refreshSettings, perform = model::settingsAction, diagnose = model::diagnoseConnections,
                messagePreviewHeight = model.messagePreviewHeight, saveMessagePreviewHeight = model::saveMessagePreviewHeight))
        }
    } else retained.SaveableStateProvider("workbench") { Workbench(
        shown.workbench,
        if (!active) WorkbenchActions() else WorkbenchActions(model::selectPeer, model::openLeader, model::openSession, model::back,
            { addDevice = true }, model::showSettings, model::retry, model::editDraft,
            model::send, model::stop, model::older, model::withdraw,
            resend = model::resend, deleteFailed = model::deleteFailed,
            comment = { row, quote -> model.conversation?.let { conversation ->
                editingComment = DraftCommentUi(NativeBridge.newId(), conversation.id, row.id,
                    row.author, row.authorAgentId.ifBlank { null }, quote, "")
            } }, editComment = { editingComment = it }, removeComment = model::removeComment,
            deviceSettings = { model.activePeer?.let { model.showDevice(it, fromChat = true) } },
            attach = { attachmentPeer = model.activePeer?.id; attachmentSession = model.conversation?.id; pickFile.launch(arrayOf("text/*", "application/json")) },
            removeAttachment = model::removeAttachment, file = { exporting = it; saveFile.launch(it.name) }, entered = model::conversationShown, message = { fullMessage = it },
            newer = model::newer, windowAnchor = model::windowAnchor, interaction = model::respondToInteraction),
    ) }
    }
    editingComment?.let { comment ->
        CommentDialog(comment, { editingComment = null }) { text -> model.saveComment(comment.copy(text = text)); editingComment = null }
    }
    if (addDevice) AddDeviceDialog(model, onDismiss = { addDevice = false })
}

private data class ClientPage(val settings: MobileSettingsState?, val workbench: WorkbenchState, val message: ChatMessage? = null)

@Composable
internal fun PlainMessage(content: String, modifier: Modifier = Modifier, preview: MessagePreviewMeasure? = null, onComment: ((String) -> Unit)? = null) {
    val textPixels = with(androidx.compose.ui.platform.LocalDensity.current) { 15.sp.toPx() }
    AndroidView(modifier = modifier, factory = { ctx ->
        object : MessageTextView(ctx) {
            override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
                super.onMeasure(widthMeasureSpec, heightMeasureSpec)
                val target = kotlin.math.ceil((layout?.lineCount ?: 1) * textSize * 1.7f).toInt()
                if (preview == null && target > measuredHeight) setMeasuredDimension(measuredWidth, resolveSize(target, heightMeasureSpec))
            }
        }.apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
            typeface = ResourcesCompat.getFont(ctx, R.font.inter)
            setTextSize(android.util.TypedValue.COMPLEX_UNIT_PX, textPixels); includeFontPadding = false; setTextColor(ZorkColors.Ink.toArgb()); setLinkTextColor(ZorkColors.Ink.toArgb())
            setTextIsSelectable(true); setLineHeight((textPixels * 1.7f).toInt())
        }
    }, update = { view ->
        view.preview = preview
        if (view.textSize != textPixels) {
            view.invalidatePreview()
            view.setTextSize(android.util.TypedValue.COMPLEX_UNIT_PX, textPixels)
            view.setLineHeight((textPixels * 1.7f).toInt())
        }
        if (view.tag != content) { view.text = content; view.tag = content; view.retainMessageText() }
        view.customSelectionActionModeCallback = if (onComment == null) null else object : android.view.ActionMode.Callback {
            override fun onCreateActionMode(mode: android.view.ActionMode, menu: android.view.Menu): Boolean {
                menu.add(0, 701, 0, "评论").setShowAsAction(android.view.MenuItem.SHOW_AS_ACTION_IF_ROOM)
                return true
            }
            override fun onPrepareActionMode(mode: android.view.ActionMode, menu: android.view.Menu) = false
            override fun onActionItemClicked(mode: android.view.ActionMode, item: android.view.MenuItem): Boolean {
                if (item.itemId != 701) return false
                val start = view.selectionStart; val end = view.selectionEnd
                if (start >= 0 && end > start) onComment(view.text.subSequence(start, end).toString())
                mode.finish(); return true
            }
            override fun onDestroyActionMode(mode: android.view.ActionMode) {}
        }
    })
}

@Composable
private fun AddDeviceDialog(model: ClientViewModel, onDismiss: () -> Unit) {
    FormDialog("连接设备", onDismiss) {
        if (model.invitation != null) PhoneInvitationStatus(model) else PhoneConnectActions(model)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FormDialog(title: String, dismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val maxHeight = (androidx.compose.ui.platform.LocalConfiguration.current.screenHeightDp * 0.85f).dp
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = state, dragHandle = null,
        shape = RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp), containerColor = ZorkColors.Canvas) {
        Column(Modifier.fillMaxWidth().heightIn(max = maxHeight).imePadding().verticalScroll(rememberScrollState()).padding(start = 22.dp, end = 22.dp, top = 12.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(title, fontSize = 19.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                IconButton(onClick = dismiss, modifier = Modifier.size(44.dp)) { Icon(painterResource(R.drawable.ic_x), "关闭", Modifier.size(22.dp), tint = ZorkColors.Ink) }
            }
            content()
        }
    }
}

@Composable
private fun CommentDialog(comment: DraftCommentUi, dismiss: () -> Unit, save: (String) -> Unit) {
    var text by remember(comment.id) { mutableStateOf(comment.text) }
    FormDialog(if (comment.text.isBlank()) "评论所选片段" else "编辑评论", dismiss) {
        Text(comment.quote, fontSize = 13.sp, lineHeight = 21.sp, modifier = Modifier.fillMaxWidth()
            .background(ZorkColors.Paper, RoundedCornerShape(8.dp)).padding(12.dp))
        Text("你的评论", fontSize = 12.sp, color = ZorkColors.Muted)
        FormField(text, { text = it }, modifier = Modifier.fillMaxWidth(), minLines = 3,
            placeholder = { Text("对这段内容有什么想法？", fontSize = 16.sp) })
        Button(onClick = { save(text) }, enabled = text.isNotBlank(), modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(10.dp)) {
            Text("加入待发送评论", fontSize = 14.sp)
        }
        Text("可以继续添加其他评论，最后和消息一起发送。", color = ZorkColors.Muted, fontSize = 12.sp)
    }
}

@Composable
internal fun FormField(value: String, onValueChange: (String) -> Unit, modifier: Modifier = Modifier,
    label: (@Composable () -> Unit)? = null, placeholder: (@Composable () -> Unit)? = null,
    singleLine: Boolean = false, minLines: Int = 1, maxLines: Int = if (singleLine) 1 else 7) {
    var focused by remember { mutableStateOf(false) }
    Column(modifier) {
        if (label != null) {
            androidx.compose.runtime.CompositionLocalProvider(LocalTextStyle provides androidx.compose.ui.text.TextStyle(
                fontFamily = ZorkFonts.Body, fontSize = 12.sp, color = ZorkColors.Muted)) { label() }
            Spacer(Modifier.height(7.dp))
        }
        androidx.compose.foundation.text.BasicTextField(value, onValueChange,
            modifier = Modifier.fillMaxWidth().heightIn(min = if (minLines > 1) 96.dp else 44.dp)
                .background(if (focused) androidx.compose.ui.graphics.Color(0xFFEEECE6) else androidx.compose.ui.graphics.Color(0xFFF6F5F1), RoundedCornerShape(9.dp))
                .onFocusChanged { focused = it.isFocused }.padding(horizontal = 12.dp, vertical = 11.dp),
            singleLine = singleLine, minLines = minLines, maxLines = maxLines,
            textStyle = androidx.compose.ui.text.TextStyle(fontFamily = ZorkFonts.Body, fontSize = 16.sp, lineHeight = 24.sp, color = ZorkColors.Ink),
            cursorBrush = androidx.compose.ui.graphics.SolidColor(ZorkColors.Ink),
            decorationBox = { inner -> Box {
                if (value.isEmpty() && placeholder != null) androidx.compose.runtime.CompositionLocalProvider(LocalTextStyle provides androidx.compose.ui.text.TextStyle(
                    fontFamily = ZorkFonts.Body, fontSize = 16.sp, lineHeight = 24.sp, color = ZorkColors.Muted)) { placeholder() }
                inner()
            } })
    }
}

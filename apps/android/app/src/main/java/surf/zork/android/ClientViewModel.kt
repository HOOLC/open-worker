package surf.zork.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.referentialEqualityPolicy
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.takeWhile
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

internal fun JSONArray?.objects(): List<JSONObject> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }
internal fun JSONObject.text(key: String, fallback: String = ""): String =
    if (isNull(key)) fallback else optString(key, fallback)

internal data class Peer(val id: String, val name: String, val address: String)
internal data class Conversation(val id: String, val title: String, val leaderId: String? = null,
    val canSend: Boolean = true, val avatar: String? = null, val canStop: Boolean = canSend)
internal data class ChatMessage(val id: String, val author: String, val content: String,
    val user: Boolean, val pending: Boolean = false, val attempted: Boolean = false,
    val avatar: String? = null, val createdAt: String = "", val device: String = "", val authorAgentId: String = "", val files: List<TextAttachmentUi> = emptyList(), val deliveryStatus: String = "", val requestId: String = "", val interaction: InteractionCardUi? = null)

internal fun parseChatMessage(it: JSONObject) = ChatMessage(it.text("id"), it.text("author_name", if (it.text("role") == "user") "用户" else "小伙伴"),
    it.text("display_content", it.text("content")), it.text("role") == "user", pending = it.optBoolean("pending"), attempted = it.optBoolean("attempted"),
    avatar = it.text("author_avatar"), createdAt = it.text("created_at"), device = it.text("device"), authorAgentId = it.text("author_agent_id"),
    files = it.textAttachments(), deliveryStatus = it.text("delivery_status"), requestId = it.text("request_id"), interaction = it.optJSONObject("interaction_card")?.let(::parseInteractionCard))

internal data class TextAttachmentUi(val id: String, val name: String, val content: String, val caption: String = "文本附件") {
    fun json(): JSONObject = JSONObject().put("id", id).put("name", name).put("content", content)
}
internal fun JSONObject.textAttachments(): List<TextAttachmentUi> = optJSONArray("attachments").objects().map {
    TextAttachmentUi(it.text("id"), it.text("name"), it.text("content"))
}

internal data class DraftCommentUi(val id: String, val session: String, val messageId: String?,
    val author: String, val authorAgentId: String?, val quote: String, val text: String) {
    fun json(): JSONObject = JSONObject().put("id", id).put("comment", text).put("source",
        JSONObject().put("session_id", session).put("message_id", messageId ?: JSONObject.NULL)
            .put("author", author).put("author_agent_id", authorAgentId ?: JSONObject.NULL).put("quote", quote))
}
internal data class DeviceTree(val leaders: List<JSONObject>, val sessions: List<JSONObject>,
    val tasksByLeader: Map<String, List<JSONObject>>, val online: Boolean = false)

internal class ClientViewModel(app: Application, private val repo: ClientRepository) : AndroidViewModel(app) {
    constructor(app: Application) : this(app, ClientRepository(app))
    var invitation by mutableStateOf<JSONObject?>(null)
        private set
    private var invitationWatch: Job? = null

    var identity by mutableStateOf("")
        private set
    var peers by mutableStateOf(emptyList<Peer>())
        private set
    var activePeer by mutableStateOf<Peer?>(null)
        private set
    var conversation by mutableStateOf<Conversation?>(null)
        private set
    var leaders by mutableStateOf(emptyList<JSONObject>())
        private set
    var sessions by mutableStateOf(emptyList<JSONObject>())
        private set
    var tasksByLeader by mutableStateOf(emptyMap<String, List<JSONObject>>())
        private set
    var comments by mutableStateOf(emptyList<DraftCommentUi>())
        private set
    var attachments by mutableStateOf(emptyList<TextAttachmentUi>())
        private set
    var participants by mutableStateOf(emptyList<JSONObject>())
        private set
    var deviceTrees by mutableStateOf(emptyMap<String, DeviceTree>())
        private set
    var settings by mutableStateOf<MobileSettingsState?>(null)
        private set
    var messagePreviewHeight by mutableIntStateOf(0)
        private set
    var running by mutableStateOf(false)
        private set
    var conversationEntry by mutableLongStateOf(0L)
        private set
    var historyLoading by mutableStateOf(false)
        private set
    private var pendingConversationLoad: (() -> Unit)? = null
    private data class CachedConversation(
        val peer: String, val id: String, val messages: List<ChatMessage>, val pending: List<ChatMessage>,
        val participants: List<JSONObject>, val draft: String, val comments: List<DraftCommentUi>,
        val attachments: List<TextAttachmentUi>, val olderCursor: String?, val historyReady: Boolean,
    )
    private var cachedConversation: CachedConversation? = null
    private fun rememberConversation() {
        val peer = activePeer ?: return
        val current = conversation ?: return
        if (messages.isEmpty() && pending.isEmpty() && (historyLoading || notice != null)) return
        cachedConversation = CachedConversation(peer.id, current.id, messages, pending, participants,
            draft, comments, attachments, olderCursor, !historyLoading)
    }
    private val messageRows = mutableStateListOf<ChatMessage>()
    var messageRevision by mutableLongStateOf(0L)
        private set
    var messages by mutableStateOf<List<ChatMessage>>(emptyList(), referentialEqualityPolicy())
        private set
    private fun replaceMessages(rows: List<ChatMessage>) {
        loadingOlder = false; hasNewer = false
        messageRows.clear(); messageRows.addAll(rows)
        messages = messageRows.toList()
        messageRevision += 1
    }
    var pending by mutableStateOf(emptyList<ChatMessage>())
        private set
    var draft by mutableStateOf("")
        private set
    var olderCursor by mutableStateOf<String?>(null)
        private set
    var hasNewer by mutableStateOf(false)
        private set
    var loadingOlder by mutableStateOf(false)
        private set
    var busy by mutableStateOf(false)
        private set
    var notice by mutableStateOf<String?>(null)
        private set
    var connected by mutableStateOf(false)
        private set
    var activity by mutableStateOf("")
        private set
    var messageActivity by mutableStateOf(MessageActivity())
        private set
    var directOnly by mutableStateOf(false)
        private set
    var ready by mutableStateOf(false)
        private set
    private var foreground = false
    private var live: Job? = null
    private var activeActions = 0

    init {
        viewModelScope.launch {
            try { messagePreviewHeight = repo.command("preferences").optInt("message_preview_height") }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { notice = "外观设置读取失败：${e.message}" }
        }
    }

    suspend fun saveMessagePreviewHeight(height: Int) {
        // Keep the committed setting visible even if the user leaves the
        // appearance page while its local write is completing.
        viewModelScope.async {
            messagePreviewHeight = repo.command("preferences", "message_preview_height" to height).getInt("message_preview_height")
        }.await()
    }

    fun foreground(value: Boolean) {
        if (foreground == value) return
        foreground = value
        live?.cancel()
        invitationWatch?.cancel()
        if (value) action {
            applySnapshot(repo.command("snapshot"))
            applySnapshot(repo.command("resume"))
            ready = true
            settings?.device?.id?.let { watchSettings(it) }
            if (foreground) { if (invitation != null) watchInvitation() else startLive() }
        } else viewModelScope.launch {
            settingsWatch?.cancel()
            runCatching { repo.command("pause") }
            connected = false
        }
    }

    private fun applySnapshot(value: JSONObject) {
        invitation = value.optJSONObject("invitation")
        identity = value.text("identity")
        directOnly = value.optJSONObject("network")?.optBoolean("direct_only") ?: false
        peers = value.optJSONArray("nodes").objects().map {
            Peer(it.text("id"), it.text("name"), it.optJSONObject("mesh")?.text("addr") ?: "")
        }
        if (activePeer == null) activePeer = peers.find { it.id == value.text("selected_peer") }
    }

    private fun action(block: suspend () -> Unit) {
        viewModelScope.launch {
            activeActions += 1
            busy = true
            notice = null
            try { block() }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { historyLoading = false; notice = error.message ?: "操作未完成，请重试" }
            finally { activeActions -= 1; busy = activeActions != 0 }
        }
    }

    fun beginInvitation(ticket: String) = action {
        live?.cancel()
        invitationWatch?.cancel()
        applySnapshot(repo.command("begin_invitation", "ticket" to ticket, "name" to android.os.Build.MODEL))
        watchInvitation()
    }

    fun cancelInvitation() = action {
        invitationWatch?.cancel()
        applySnapshot(repo.command("cancel_invitation"))
        startLive()
    }

    private fun watchInvitation() {
        invitationWatch?.cancel()
        invitationWatch = viewModelScope.launch {
            try {
                repo.invitationEvents().collect { value ->
                    applySnapshot(value)
                    notice = value.text("notice").takeIf { it.isNotBlank() }
                    if (value.has("joined_peer")) peers.find { it.id == value.text("joined_peer") }?.let { selectPeer(it) }
                }
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { notice = error.message }
        }
    }

    fun changeDirectOnly(enabled: Boolean) = action {
        live?.cancel()
        applySnapshot(repo.command("network", "network" to JSONObject().put("direct_only", enabled)))
        startLive()
    }

    fun selectPeer(peer: Peer) {
        rememberConversation()
        pendingConversationLoad = null
        historyLoading = false
        live?.cancel()
        activePeer = peer
        conversation = null
        leaders = emptyList(); sessions = emptyList(); tasksByLeader = emptyMap(); replaceMessages(emptyList()); pending = emptyList()
        connected = false; notice = null; activity = ""
        action { repo.command("select_peer", "peer" to peer.id); startLive() }
    }

    fun back() {
        rememberConversation()
        pendingConversationLoad = null
        historyLoading = false
        live?.cancel()
        if (conversation != null) {
            conversation = null; replaceMessages(emptyList()); pending = emptyList(); activity = ""
            action { startLive() }
        } else {
            activePeer = null
            action { repo.command("select_peer", "peer" to null) }
        }
    }

    fun openLeader(leader: JSONObject) {
        rememberConversation()
        val peer = peers.find { it.id == leader.text("_peer") } ?: activePeer ?: return
        if (activePeer?.id != peer.id) { live?.cancel(); activePeer = peer; conversation = null; connected = false }
        val sessionId = leader.text("session_id")
        if (sessionId.isNotBlank()) {
            // The list already identifies this conversation. Show it and its local
            // history before waiting for the remote runtime to be prepared.
            openConversation(Conversation(sessionId, leader.text("name"), leader.text("id"), avatar = leader.text("avatar")),
                prepareLeader = leader.text("id"))
            return
        }
        action {
            val opened = repo.command("settings_action", "peer" to peer.id, "operation" to JSONObject().put("action", "open_agent").put("id", leader.text("id")))
            openConversation(Conversation(opened.text("session_id"), leader.text("name"), leader.text("id"), avatar = leader.text("avatar")))
        }
    }

    fun openSession(session: JSONObject) {
        rememberConversation()
        peers.find { it.id == session.text("_peer") }?.let { peer -> if (activePeer?.id != peer.id) { live?.cancel(); activePeer = peer; conversation = null; connected = false } }
        val task = session.optJSONObject("task")
        openConversation(Conversation(session.text("session_id"), task?.text("title", "任务") ?: "对话",
            canSend = session.optBoolean("can_send", true), canStop = session.optBoolean("can_stop")))
    }

    private fun openConversation(value: Conversation, preparedDraft: String? = null, prepareLeader: String? = null) {
        if (value.id.isBlank()) return
        rememberConversation()
        val cached = cachedConversation?.takeIf { it.peer == activePeer?.id && it.id == value.id }
        live?.cancel()
        conversation = value
        conversationEntry += 1
        messageActivity = MessageActivity()
        historyLoading = cached?.historyReady != true
        replaceMessages(cached?.messages.orEmpty()); pending = cached?.pending.orEmpty()
        comments = cached?.comments.orEmpty(); attachments = cached?.attachments.orEmpty()
        participants = cached?.participants.orEmpty(); draft = cached?.draft.orEmpty()
        olderCursor = cached?.olderCursor; activity = ""
        val peerId = activePeer?.id
        pendingConversationLoad = { action {
            if (conversation !== value || activePeer?.id != peerId) return@action
            loadLocalConversation()
            if (conversation !== value || activePeer?.id != peerId) return@action
            if (preparedDraft != null) editDraft(listOf(draft, preparedDraft).filter { it.isNotBlank() }.joinToString("\n"))
            startLive(prepareLeader)
        } }
    }

    fun conversationShown() {
        val load = pendingConversationLoad ?: return
        pendingConversationLoad = null
        load()
    }

    private suspend fun loadLocalConversation() {
        val peer = activePeer ?: return
        val current = conversation ?: return
        val result = repo.command("conversation", "peer" to peer.id, "session" to current.id)
        if (peer != activePeer || current != conversation) return
        draft = result.text("draft")
        attachments = result.textAttachments()
        comments = result.optJSONArray("comments").objects().map { c ->
            val source = c.getJSONObject("source")
            DraftCommentUi(c.text("id"), source.text("session_id"), source.text("message_id").ifBlank { null },
                source.text("author"), source.text("author_agent_id").ifBlank { null }, source.text("quote"), c.text("comment"))
        }
    }

    fun editDraft(value: String) {
        draft = value
        val peer = activePeer ?: return
        val current = conversation ?: return
        viewModelScope.launch {
            try { repo.command("draft", "peer" to peer.id, "session" to current.id, "content" to value) }
            catch (error: Exception) { if (error !is CancellationException) notice = "草稿尚未保存：${error.message}" }
        }
    }

    fun send() {
        val peer = activePeer ?: return
        val current = conversation ?: return
        val content = draft
        if (busy || !current.canSend) return
        action {
            repo.command("submit_draft", "peer" to peer.id, "session" to current.id, "text" to content)
            loadLocalConversation()
        }
    }

    fun respondToInteraction(messageId: String, choice: String, values: Map<String, String>) {
        val peer = activePeer ?: return
        val current = conversation ?: return
        action {
            repo.command("respond_to_interaction", "peer" to peer.id, "session" to current.id,
                "operation" to JSONObject().put("action", "activate").put("message_id", messageId)
                    .put("choice", choice).put("values", JSONObject(values)))
        }
    }

    suspend fun diagnoseConnections(): List<DeviceDiagnosis> = repo.command("diagnose_connections")
        .optJSONArray("items").objects().map { DeviceDiagnosis(it.text("name"), it.optBoolean("reachable")) }

    private var settingsWatch: Job? = null
    private fun watchSettings(peer: String) {
        settingsWatch?.cancel()
        settingsWatch = viewModelScope.launch {
            try {
                repo.settingsEvents(peer).takeWhile { foreground && settings?.device?.id == peer }.collect { frame ->
                    val result = frame.value
                    if (result.optBoolean("changed")) result.optJSONObject("snapshot")?.let {
                        applySettingsSnapshot(peer, it, settings?.loading == true)
                    }
                }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { if (settings?.device?.id == peer) settings = settings?.copy(message = e.message) }
        }
    }
    private fun applySettingsSnapshot(peer: String, snapshot: JSONObject, refreshing: Boolean) {
        val current = settings?.takeIf { it.device?.id == peer } ?: return
        if (snapshot.optBoolean("revoked")) {
            settings = current.copy(info = null, agents = emptyList(), profiles = emptyList(), providers = emptyList(), profile = null,
                authorization = null, authorizationBusy = false, authorizationComplete = false, authorizationError = null, operation = null,
                loading = false, online = false, profilesReady = false, message = "设备访问权限已撤销")
            return
        }
        if (!snapshot.optBoolean("ready")) {
            settings = current.copy(loading = refreshing, message = snapshot.text("error").takeIf { it.isNotBlank() },
                online = snapshot.optBoolean("online", current.online))
            return
        }
        val info = snapshot.optJSONObject("info") ?: JSONObject()
        val profiles = snapshot.optJSONArray("profiles").objects()
        val selectedId = current.profile?.text("profile_id")
        val name = info.text("name").takeIf { it.isNotBlank() }
        settings = current.copy(authorization = snapshot.optJSONObject("authorization"), authorizationComplete = snapshot.optBoolean("authorization_complete"),
            authorizationBusy = snapshot.optBoolean("authorization_busy"), authorizationError = snapshot.text("authorization_error").takeIf { it.isNotBlank() },
            operation = snapshot.optJSONObject("operation"), info = info, device = name?.let { current.device?.copy(name = it) } ?: current.device,
            agents = snapshot.optJSONArray("agents").objects(), profiles = profiles, providers = snapshot.optJSONArray("providers").objects(),
            profilesReady = snapshot.optBoolean("profiles_ready", true),
            profileMessage = snapshot.text("profiles_error").takeIf { it.isNotBlank() }?.let { "模型连接暂时无法刷新，已保留本地内容" },
            profile = profiles.find { it.text("profile_id") == selectedId }, loading = refreshing,
            online = snapshot.optBoolean("online", false), message = snapshot.text("error").takeIf { it.isNotBlank() })
    }
    fun showSettings() { settings = MobileSettingsState() }
    fun showDevice(peer: Peer, fromChat: Boolean = false) {
        val tree = deviceTrees[peer.id]
        settings = MobileSettingsState(page = "device", device = peer, fromChat = fromChat,
            online = tree?.online ?: false, agents = tree?.leaders.orEmpty(), loading = true, profilesReady = false)
        refreshSettings()
        watchSettings(peer.id)
    }
    suspend fun settingsAction(action: String, fields: JSONObject): JSONObject {
        val peer=settings?.device ?: error("请先选择设备")
        return repo.command("settings_action", "peer" to peer.id, "operation" to fields.put("action", action))
    }
    fun refreshSettings() {
        val previous = settings ?: return
        val peer = previous.device ?: return
        settings = previous.copy(loading = true, message = null)
        action {
            try {
                applySettingsSnapshot(peer.id, repo.command("settings", "peer" to peer.id, "cached_only" to true), true)
                applySettingsSnapshot(peer.id, repo.command("settings", "peer" to peer.id), false)
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                if (settings?.device?.id == peer.id) settings = settings?.copy(loading = false, message = e.message, online = false)
            }
        }
    }
    fun settingsPage(page: String) { settings = settings?.copy(page = page) }
    fun settingsProfile(profile: JSONObject) { settings = settings?.copy(page = "profile", profile = profile); refreshSettings() }
    fun backSettings() {
        settings = when (settings?.page) {
            "home" -> null
            "appearance", "display", "diagnostics", "about" -> MobileSettingsState()
            "device" -> if (settings?.fromChat == true) null else MobileSettingsState()
            "profile" -> settings?.copy(page = "models")
            else -> settings?.copy(page = "device")
        }
    }
    fun checkUpdate() = action {
        val peer = settings?.device ?: return@action
        val result = repo.command("settings_action", "peer" to peer.id, "operation" to JSONObject().put("action", "check_update"))
        if (settings?.device?.id == peer.id) settings = settings?.copy(message = result.text("latest_version").takeIf { it.isNotBlank() }?.let { "最新版本：$it" } ?: "尚未获得版本信息")
    }
    fun assistSettings(leader: JSONObject) = action {
        val targetDevice = settings?.device ?: return@action
        val peer = peers.find { it.id == leader.text("_peer") } ?: targetDevice
        settings = null
        if (activePeer?.id != peer.id) { activePeer = peer; conversation = null }
        val opened = repo.command("settings_action", "peer" to peer.id, "operation" to JSONObject().put("action", "open_agent").put("id", leader.text("id")))
        openConversation(Conversation(opened.text("session_id"), leader.text("name"), leader.text("id"), avatar = leader.text("avatar")),
            preparedDraft = "帮我看看 ${targetDevice.name} 的设备配置。")
    }

    fun addTextAttachment(uri: android.net.Uri, peer: String, session: String) = action {
        repo.attachText(uri, peer, session)
    }
    fun removeAttachment(id: String) = draftAction(JSONObject().put("action", "remove_attachment").put("id", id))
    fun exportTextAttachment(uri: android.net.Uri, file: TextAttachmentUi) = action {
        repo.exportText(uri, file.content)
        notice = "已保存 ${file.name}"
    }
    private fun draftAction(operation: JSONObject) = action {
        val peer = activePeer ?: return@action
        val current = conversation ?: return@action
        repo.command("draft_action", "peer" to peer.id, "session" to current.id, "operation" to operation)
    }

    fun assistAddDevice(leader: JSONObject) = action {
        val peer = peers.find { it.id == leader.text("_peer") } ?: activePeer ?: return@action
        settings = null; if (activePeer?.id != peer.id) { live?.cancel(); activePeer = peer; conversation = null }
        val opened = repo.command("settings_action", "peer" to peer.id, "operation" to JSONObject().put("action", "open_agent").put("id", leader.text("id")))
        openConversation(Conversation(opened.text("session_id"), leader.text("name"), leader.text("id"), avatar = leader.text("avatar")),
            preparedDraft = "我想添加一台新设备，请帮我准备接入步骤。")
    }

    fun saveComment(comment: DraftCommentUi) = draftAction(JSONObject().put("action", "put_comment").put("comment", comment.json()))
    fun removeComment(id: String) = draftAction(JSONObject().put("action", "remove_comment").put("id", id))

    fun withdraw(id: String) = action {
        val peer = activePeer ?: return@action
        repo.command("withdraw", "peer" to peer.id, "request_id" to id)
        loadLocalConversation()
    }

    fun resend(id: String) = action {
        val peer = activePeer ?: return@action
        repo.command("retry", "peer" to peer.id, "request_id" to id)
        loadLocalConversation()
    }
    fun deleteFailed(id: String) = action {
        val peer = activePeer ?: return@action
        repo.command("delete_failed", "peer" to peer.id, "request_id" to id)
        loadLocalConversation()
    }

    fun retry() = action { repo.command("resume"); startLive() }

    fun stop() = action {
        val peer = activePeer ?: return@action
        val current = conversation ?: return@action
        repo.command("settings_action", "peer" to peer.id, "operation" to JSONObject().put("action", "stop_conversation").put("session", current.id))
    }

    fun older() = action {
        val peer = activePeer ?: return@action
        val current = conversation ?: return@action
        repo.older(peer.id, current.id)
    }
    fun newer() = action {
        val peer = activePeer ?: return@action
        val current = conversation ?: return@action
        repo.newer(peer.id, current.id)
    }
    fun windowAnchor(anchor: String?) {
        val peer = activePeer ?: return
        val current = conversation ?: return
        viewModelScope.launch {
            try { repo.windowAnchor(peer.id, current.id, anchor) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { if (activePeer?.id == peer.id && conversation?.id == current.id) notice = error.message }
        }
    }

    private fun startLive(prepareLeader: String? = null) {
        live?.cancel()
        val peer = activePeer ?: return
        if (!foreground) return
        val current = conversation
        live = viewModelScope.launch {
            try {
                var initial = true
                repo.events(peer.id, current?.id).takeWhile { foreground && activePeer?.id == peer.id && conversation?.id == current?.id }.collect { frame ->
                    Snapshot.withMutableSnapshot { applyState(frame) }
                    if (initial) {
                        initial = false
                        if (prepareLeader != null) launch { repo.command("settings_action", "peer" to peer.id,
                            "operation" to JSONObject().put("action", "prepare_agent").put("id", prepareLeader)) }
                    }
                }
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                connected = false
                historyLoading = false
                notice = error.message ?: "客户端连接未建立，请重试"
            }
        }
    }

    // Rust owns connection recovery, history, message identity, permissions and
    // outbox delivery. Kotlin maps the snapshot to display models only.
    private fun applyState(frame: ObservationFrame) {
        val state = frame.value.optJSONObject("state")
        if (state == null || state.text("peer") != activePeer?.id ||
            state.text("session") != (conversation?.id ?: "")) return
        if(state.has("nodes")) {
            val names=state.optJSONArray("nodes").objects().associate { it.text("id") to it.text("name") }
            peers=peers.map { p -> names[p.id]?.takeIf{it.isNotBlank()}?.let{p.copy(name=it)} ?: p }
            activePeer=activePeer?.let{p -> names[p.id]?.takeIf{it.isNotBlank()}?.let{p.copy(name=it)} ?: p}
            settings=settings?.let{current -> current.copy(device=current.device?.let{p -> names[p.id]?.takeIf{it.isNotBlank()}?.let{p.copy(name=it)} ?: p})}
        }
        state.optJSONObject("message_arrivals")?.let { arrivals ->
            val count = arrivals.optLong("count")
            if (count > 0) {
                val sequence = messageActivity.sequence + count
                val ids = arrivals.optJSONArray("ids")
                val size = ids?.length() ?: 0
                val now = android.os.SystemClock.uptimeMillis()
                val recent = messageActivity.recent.filter { now - it.startedAt < 250 } +
                    (0 until size).map { MessageArrival(ids!!.getString(it), sequence - size + it + 1, now) }
                messageActivity = MessageActivity(sequence, recent.takeLast(32))
            }
        }
        if (state.has("connected")) connected = state.optBoolean("connected")
        if (state.has("error")) notice = state.text("error").ifBlank { null }
        if (state.has("agents")) leaders = state.optJSONArray("agents").objects().filter { it.text("role") == "leader" }
        if (state.has("sessions")) sessions = state.optJSONArray("sessions").objects().filter { it.optJSONObject("task") != null }
        frame.messages?.let(::replaceMessages)
        for (edit in frame.edits) {
            check(edit.start >= 0 && edit.end >= edit.start && edit.end <= messageRows.size) { "消息增量超出已应用范围" }
            messageRows.subList(edit.start, edit.end).clear()
            messageRows.addAll(edit.start, edit.insert)
        }
        if (frame.edits.isNotEmpty()) { messages = messageRows.toList(); messageRevision += 1 }
        if (state.optBoolean("unified_transcript")) pending = emptyList()
        if (messages.isNotEmpty() || state.optBoolean("loaded") || notice != null) historyLoading = false
        if (state.has("loading_older")) loadingOlder = state.optBoolean("loading_older")
        if (state.has("newer_available")) hasNewer = state.optBoolean("newer_available")
        state.optJSONObject("draft_document")?.let { document ->
            attachments = document.textAttachments()
            comments = document.optJSONArray("comments").objects().map { c ->
                val source = c.getJSONObject("source")
                DraftCommentUi(c.text("id"), source.text("session_id"), source.text("message_id").ifBlank { null },
                    source.text("author"), source.text("author_agent_id").ifBlank { null }, source.text("quote"), c.text("comment"))
            }
        }
        if (state.has("tasks_by_leader")) {
            val taskGroups = state.optJSONObject("tasks_by_leader")
            tasksByLeader = taskGroups?.keys()?.asSequence()?.associateWith { taskGroups.optJSONArray(it).objects() } ?: emptyMap()
        }
        if (state.has("participants")) participants = state.optJSONArray("participants").objects()
        if (state.has("agents") || state.has("sessions") || state.has("tasks_by_leader") || state.has("connected"))
            activePeer?.let { deviceTrees = deviceTrees + (it.id to DeviceTree(leaders, sessions, tasksByLeader, connected)) }
        if (state.has("running")) running = state.optBoolean("running")
        if (state.has("older_cursor")) olderCursor = state.text("older_cursor").ifBlank { null }
        if (state.has("can_send")) conversation = conversation?.copy(canSend = state.optBoolean("can_send"), canStop = state.optBoolean("can_stop"))
        if (state.has("activity")) activity = if (state.optBoolean("stop_pending")) "已请求停止，等待设备确认" else
            activityLabel(state.optJSONObject("activity"))
    }
}

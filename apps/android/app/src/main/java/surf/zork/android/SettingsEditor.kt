package surf.zork.android

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun SettingsEditor(kind: String, source: JSONObject?, state: MobileSettingsState, actions: SettingsActions, latest: String, dismiss: () -> Unit, saved: () -> Unit, manageGrants: (JSONObject) -> Unit = {}) {
    if (kind == "model") { ModelEditor(source, state, actions, dismiss, saved); return }
    val scope=rememberCoroutineScope();val context=LocalContext.current
    var busy by remember { mutableStateOf(false) };var error by remember { mutableStateOf<String?>(null) }
    var name by remember { mutableStateOf(if(kind=="rename") state.device?.name.orEmpty() else if(kind=="profile-name") source?.let(::profileName).orEmpty() else source?.text("name").orEmpty()) }
    var avatar by remember { mutableStateOf(source?.text("avatar")?.ifBlank { "cat" } ?: "cat") }
    var role by remember { mutableStateOf(source?.text("role") ?: "leader") }
    var profileId by remember { mutableStateOf(source?.text("profile_id")?.ifBlank { if(kind=="agent") "auto" else "" } ?: if(kind=="agent") "auto" else "") }
    var agentThinking by remember { mutableStateOf(source?.text("thinking") ?: "") }
    var modelId by remember { mutableStateOf(source?.text(if(kind=="agent")"model"else"id") ?: "") }
    var instructions by remember { mutableStateOf("") }
    val initialGrants = remember(source) { source?.optJSONArray("allowed_leaders")?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty() }
    var selectedGrants by remember { mutableStateOf(initialGrants.filter { '/' !in it }.toSet()) }
    var remoteGrants by remember { mutableStateOf(initialGrants.filter { '/' in it }.joinToString("\n")) }
    fun grants() = JSONArray((selectedGrants + remoteGrants.split(Regex("\\s+")).filter { it.isNotBlank() }).toList())
    var access by remember { mutableStateOf("subscription") }
    var providerId by remember { mutableStateOf("") }
    var billingId by remember { mutableStateOf("") }
    var key by remember { mutableStateOf("") };var base by remember { mutableStateOf("") }
    val attempt = state.authorization
    var callback by remember { mutableStateOf("") }
    var authorizationStarted by remember { mutableStateOf(false) }
    var upgradeId by remember { mutableStateOf<String?>(null) }
    var progress by remember { mutableStateOf("") }
    val connectionOptions = remember(state.providers, access, providerId, billingId) {
        JSONObject(NativeBridge.connectionChoices(JSONArray(state.providers).toString(), access == "subscription", providerId, billingId))
    }
    val providers = connectionOptions.optJSONArray("providers").objects()
    val provider = connectionOptions.optJSONObject("provider")
    val billings = connectionOptions.optJSONArray("billings").objects()
    val billing = connectionOptions.optJSONObject("billing")
    val deviceCode=billing?.optBoolean("deviceCode")==true
    val id=remember { NativeBridge.newId() }
    fun submit(work: suspend () -> Unit) { scope.launch { busy=true;error=null;try{work()}catch(e:CancellationException){throw e}catch(e:Exception){error=e.message ?: "操作未完成，请重试"}finally{busy=false} } }
    fun close() { if(attempt!=null) submit { actions.perform("cancel_authorization", JSONObject()); dismiss() } else dismiss() }
    LaunchedEffect(state.authorizationError) { state.authorizationError?.let { error = it } }
    LaunchedEffect(state.authorizationComplete) {
        if (authorizationStarted && state.authorizationComplete) { key = ""; saved() }
    }
    LaunchedEffect(state.operation) {
        val operation = state.operation
        if (operation != null && upgradeId != null && operation.text("id") == upgradeId) {
            progress = operation.text("message")
            if (operation.optBoolean("completed")) saved()
            operation.text("error").takeIf { it.isNotBlank() }?.let { error = it }
        }
    }
    val title=when(kind){"rename"->"修改设备名称";"agent"->if(source==null)"添加小伙伴"else"编辑小伙伴";"connection"->"添加模型连接";"profile-name"->"重命名连接";"grants"->"管理领队授权";else->"升级设备"}
    SettingsSheet(title,busy,error,{close()}) {
        when(kind){
            "grants" -> {
                Text(source?.text("name").orEmpty(), fontSize = 16.sp)
                AgentGrantFields(state, selectedGrants, remoteGrants, !busy, { selectedGrants = it }, { remoteGrants = it })
                SettingsButton(if (busy) "保存中…" else "保存授权", true, !busy && state.online) { submit {
                    actions.perform("agent_grants", JSONObject().put("id", source!!.text("id"))
                        .put("allowed", grants()).put("expected", JSONArray(initialGrants)))
                    saved()
                } }
            }
            "profile-name" -> {
                SettingsField("名称", name, { name = it }, enabled = !busy)
                Text("名称可使用中文，原有连接 ID 和小伙伴配置保持有效。", fontSize = 12.sp, color = ZorkColors.Muted)
                SettingsButton(if (busy) "保存中…" else "保存", true, !busy && state.online) { submit {
                    actions.perform("rename_profile", JSONObject().put("profile", source!!.text("profile_id")).put("name", name))
                    saved()
                } }
            }
            "rename"->{
                SettingsField("名称",name,{name=it},enabled=!busy)
                Text("连接此设备的小伙伴都会看到新名称。",fontSize=12.sp,color=ZorkColors.Muted)
                SettingsButton(if(busy)"保存中…"else"保存",true,!busy){submit{actions.perform("rename_device",JSONObject().put("name",name));saved()}}
            }
            "agent"->{
                if(source==null){SettingsSegments(listOf("leader" to "领队","worker" to "队员"),role,!busy){role=it};SettingsField("名称",name,{name=it},enabled=!busy)}else Text(source.text("name"),fontSize=16.sp)
                fun choices(model: String = modelId, thinking: String = agentThinking) = JSONObject(
                    NativeBridge.agentChoices(JSONArray(state.profiles).toString(), profileId, model, thinking))
                val options = remember(state.profiles, profileId, modelId, agentThinking) { choices() }
                val models = options.optJSONArray("models").objects()
                val levels = options.optJSONArray("levels")?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()
                fun select(model: String, thinking: String) {
                    val repaired = choices(model, thinking)
                    modelId = model; profileId = repaired.text("profile"); agentThinking = repaired.text("thinking")
                }
                SettingsSelect("模型", modelId, models.map { it.text("id") to it.text("id") }, !busy) { select(it, agentThinking) }
                SettingsSelect("思考深度", agentThinking, levels.map { it to it }, !busy && levels.isNotEmpty()) { select(modelId, it) }
                SettingsSelect("模型连接（可选）", profileId, options.optJSONArray("profiles").objects().map { it.text("profile_id") to profileName(it) }, !busy) { profileId = it }
                Text("头像",fontSize=12.sp,color=ZorkColors.Muted)
                FlowRow(horizontalArrangement=Arrangement.spacedBy(4.dp),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                    listOf("cat","bunny","bear","fox","panda","chick","dog","owl","koala","penguin","deer","octopus").forEach { value ->
                        Surface(onClick={avatar=value},enabled=!busy,shape=SettingsStyle.Field,color=if(avatar==value)ZorkColors.Selected else ZorkColors.Canvas){Box(Modifier.size(48.dp),contentAlignment=Alignment.Center){Avatar(value,32.dp,value)}}
                    }
                }
                if(source==null) SettingsField("职责与偏好 · 可选",instructions,{instructions=it},enabled=!busy,singleLine=false)
                if(source==null && role=="worker") AgentGrantFields(state, selectedGrants, remoteGrants, !busy, { selectedGrants = it }, { remoteGrants = it })
                if(source?.text("role")=="worker") SettingsButton("管理授权",enabled=!busy){manageGrants(source)}
                if(state.profiles.isEmpty()) Text("先到「大模型」添加连接与模型。",fontSize=13.sp,color=ZorkColors.Muted)
                SettingsButton(if(busy)"保存中…"else if(source==null)"创建"else"保存修改",true,!busy && options.optBoolean("valid")) {submit{
                    actions.perform("save_agent", JSONObject().put("input", JSONObject()
                        .put("id", source?.text("id") ?: id).put("creating", source == null).put("name", name)
                        .put("role", role).put("avatar", avatar).put("profile", profileId).put("model", modelId)
                        .put("thinking", agentThinking).put("instructions", instructions).put("allowed", grants())))
                    saved()
                }}
            }
            "connection"->{
                SettingsSegments(listOf("subscription" to "订阅账号","api" to "API 接入"),access,!busy && attempt==null){access=it;providerId="";billingId="";key=""}
                SettingsSelect("提供商",providerId,providers.map{it.text("id") to it.text("label",it.text("id"))},!busy && attempt==null){providerId=it;billingId="";base="";key=""}
                if(billings.size>1) SettingsSelect("接入方式",billing?.text("id").orEmpty(),billings.map{it.text("id") to it.text("label")},!busy && attempt==null){billingId=it;key=""}
                SettingsField("连接名称",profileId,{profileId=it},enabled=!busy && attempt==null)
                if(!deviceCode && provider!=null && billing!=null){
                    if(providerId=="openai-compatible") SettingsField("接口地址",base,{base=it},enabled=!busy)
                    SettingsField("API Key",key,{key=it},secret=true,enabled=!busy)
                }
                attempt?.let { pending ->
                    val code=pending.text("user_code");Text(if(code.isNotBlank())"在浏览器输入：$code"else"在浏览器完成授权，再粘贴返回内容。",fontSize=14.sp)
                    SettingsButton("打开登录页面"){val uri=Uri.parse(pending.text("verification_url"));if(uri.scheme=="https") context.startActivity(Intent(Intent.ACTION_VIEW,uri))}
                    if(pending.text("flow")=="browser_callback") SettingsField("浏览器返回内容",callback,{callback=it},enabled=!busy)
                }
                SettingsButton(if(busy)"处理中…"else if(provider==null)"选择提供商"else if(deviceCode)if(attempt==null)"登录并连接"else"完成连接"else"保存连接",true,!busy && provider!=null && billing!=null && (attempt==null || attempt?.text("flow")=="browser_callback")){submit{
                    if(deviceCode) {
                        authorizationStarted = true
                        if(attempt == null) actions.perform("start_authorization", JSONObject().put("profile", profileId).put("provider", providerId).put("billing", billing!!.text("id")))
                        else actions.perform("complete_authorization", JSONObject().put("callback", callback))
                    } else {
                        actions.perform("save_connection", JSONObject().put("input", JSONObject().put("id", profileId)
                            .put("provider", providerId).put("billing", billing!!.text("id")).put("base_url", base).put("key", key)))
                        key = ""; saved()
                    }
                }}
            }
            "upgrade"->{
                Text("升级至 $latest 后设备会重启，连接将暂时中断。",fontSize=14.sp,lineHeight=22.sp)
                if(progress.isNotBlank()) Text(progress,fontSize=13.sp,color=ZorkColors.Muted)
                SettingsButton("确认升级",true,!busy){submit{
                    upgradeId = actions.perform("upgrade", JSONObject().put("version", latest)).text("operation")
                }}
            }
        }
    }
}

package surf.zork.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

internal data class InteractionFieldUi(val id: String, val label: String, val kind: String,
    val value: String, val options: List<Pair<String, String>>, val error: String?)
internal data class InteractionActionUi(val id: String, val label: String, val primary: Boolean)
internal data class InteractionCardUi(val id: String, val title: String, val status: String,
    val fields: List<InteractionFieldUi>, val details: List<Pair<String, String>>,
    val actions: List<InteractionActionUi>, val editable: Boolean, val error: String?)

// Presentation strings only. Action availability, validation and result folding
// are supplied by Rust core, never inferred from these labels.
private fun interactionText(key: String) = when (key) {
    "interaction_name" -> "名称"
    "interaction_instructions" -> "职责与要求"
    "interaction_model" -> "模型连接 · 模型 · 思考深度"
    "interaction_skills" -> "技能"
    "interaction_grants" -> "可调用此队员的小伙伴"
    "interaction_create_agent" -> "创建执行队员"
    "interaction_update_agent" -> "修改小伙伴配置"
    "interaction_confirm_create" -> "确认创建"
    "interaction_confirm_update" -> "确认修改"
    "interaction_submit" -> "提交"
    "interaction_decline" -> "暂不执行"
    "interaction_retry" -> "恢复原提交"
    "interaction_completed" -> "已完成"
    "interaction_declined" -> "已拒绝"
    "interaction_unconfirmed" -> "结果待确认"
    "interaction_submitting" -> "正在提交…"
    "interaction_confirmation" -> "等待你的确认"
    "interaction_unsupported" -> "当前版本暂不支持此操作"
    "input_required" -> "请填写此项"
    "input_too_large" -> "输入内容过长"
    "invalid_input_choice" -> "请选择有效选项"
    "unknown_input_field" -> "不支持的输入字段"
    "invalid_agent_name" -> "请填写有效名称"
    "invalid_agent_configuration" -> "请检查配置内容"
    "invalid_agent_resources" -> "请检查资源配置"
    else -> key
}

internal fun parseInteractionCard(value: JSONObject): InteractionCardUi = InteractionCardUi(
    value.text("message_id"), value.text("title").let { if (value.optBoolean("localized_title")) interactionText(it) else it },
    interactionText(value.text("status_key")),
    value.optJSONArray("fields").objects().map { item ->
        val field = item.getJSONObject("field")
        InteractionFieldUi(field.text("id"), field.text("label").let { if (item.optBoolean("localized_label")) interactionText(it) else it },
            field.text("kind", "text"), item.text("value"), field.optJSONArray("options").objects().map { it.text("value") to it.text("label") },
            item.text("error_key").takeIf(String::isNotBlank)?.let(::interactionText))
    },
    value.optJSONArray("details").objects().map { interactionText(it.text("label_key")) to it.text("value") },
    value.optJSONArray("actions").objects().map { InteractionActionUi(it.text("id"), interactionText(it.text("label_key")), it.optBoolean("primary")) },
    value.optBoolean("editable"), value.text("error").takeIf(String::isNotBlank),
)

@Composable
internal fun InteractionCard(card: InteractionCardUi, activate: (String, Map<String, String>) -> Unit) {
    val values = remember(card.id) { mutableStateMapOf<String, String>() }
    Surface(modifier = Modifier.fillMaxWidth().widthIn(max = 620.dp), shape = RoundedCornerShape(12.dp),
        color = ZorkColors.Canvas, border = BorderStroke(1.dp, ZorkColors.Border)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(card.title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = ZorkColors.Ink)
                Text(card.status, fontSize = 12.sp, color = ZorkColors.Muted)
            }
            card.fields.forEach { field -> key(card.id, field.id) {
                var draft by rememberSaveable(card.id, field.id) { mutableStateOf(field.value) }
                var open by remember { mutableStateOf(false) }
                LaunchedEffect(card.editable, field.value) {
                    if (!card.editable) draft = field.value
                    values[field.id] = draft
                }
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(field.label, fontSize = 12.sp, color = ZorkColors.Muted)
                    if (!card.editable) {
                        Text(field.options.find { it.first == field.value }?.second ?: field.value,
                            fontSize = 13.sp, color = ZorkColors.Ink)
                    } else if (field.kind == "choice") {
                        Box {
                            OutlinedButton(onClick = { open = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)
                                .semantics { contentDescription = field.label }) {
                                Text(field.options.find { it.first == draft }?.second ?: "请选择")
                            }
                            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                                field.options.forEach { (value, label) -> DropdownMenuItem(text = { Text(label) }, onClick = {
                                    draft = value; values[field.id] = value; open = false
                                }) }
                            }
                        }
                    } else {
                        OutlinedTextField(value = draft, onValueChange = { draft = it; values[field.id] = it },
                            modifier = Modifier.fillMaxWidth().semantics { contentDescription = field.label },
                            singleLine = field.kind != "multiline", minLines = if (field.kind == "multiline") 3 else 1,
                            maxLines = if (field.kind == "multiline") 5 else 1, isError = field.error != null,
                            shape = RoundedCornerShape(8.dp))
                    }
                    field.error?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 12.sp) }
                }
            } }
            if (card.details.isNotEmpty()) {
                HorizontalDivider(color = ZorkColors.Border, thickness = .5.dp)
                card.details.forEach { (label, value) -> Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(label, fontSize = 11.sp, color = ZorkColors.Muted)
                    Text(value, fontSize = 12.sp, color = ZorkColors.Ink)
                } }
            }
            card.error?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 12.sp) }
            card.actions.forEach { action ->
                val click = { activate(action.id, values.toMap()) }
                if (action.primary) Button(onClick = click, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = ZorkColors.Ink), shape = RoundedCornerShape(8.dp)) { Text(action.label) }
                else TextButton(onClick = click, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(action.label) }
            }
        }
    }
}

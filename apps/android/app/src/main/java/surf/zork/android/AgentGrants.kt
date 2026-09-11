package surf.zork.android

import androidx.compose.foundation.layout.*
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AgentGrantFields(state: MobileSettingsState, selected: Set<String>, remote: String, enabled: Boolean,
    select: (Set<String>) -> Unit, changeRemote: (String) -> Unit) {
    Text("单独授权领队", fontSize = 14.sp)
    Text("同一 mesh 内的领队默认可以指派任务。此列表用于其余单独授权。", fontSize = 12.sp, color = ZorkColors.Muted)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        state.agents.filter { it.text("role") == "leader" }.forEach { leader ->
            val id = leader.text("id")
            FilterChip(selected = id in selected, onClick = { select(if (id in selected) selected - id else selected + id) },
                enabled = enabled, label = { Text(leader.text("name"), fontSize = 13.sp) })
        }
    }
    SettingsField("其他设备的领队引用 · 可选", remote, changeRemote, enabled = enabled, singleLine = false,
        detail = "多个引用用空格或换行分隔")
}

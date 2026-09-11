package surf.zork.android

import androidx.compose.foundation.*
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Hallmark · existing Zork system · touch targets 44dp · white, warm surfaces, Inter.
internal object SettingsStyle {
    val Card = RoundedCornerShape(20.dp)
    val Field = RoundedCornerShape(12.dp)
    val Pill = RoundedCornerShape(50)
}
@Composable internal fun SettingsButton(text: String, primary: Boolean = false, enabled: Boolean = true, click: () -> Unit) {
    if (primary) Button(modifier = Modifier.fillMaxWidth(), onClick = click, enabled = enabled, shape = SettingsStyle.Pill, contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp)) { Text(text, maxLines = 1, fontSize = 14.sp) }
    else OutlinedButton(onClick = click, enabled = enabled, shape = SettingsStyle.Pill, border = BorderStroke(1.dp, ZorkColors.FieldBorder), contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp)) { Text(text, maxLines = 1, fontSize = 14.sp) }
}
@Composable internal fun SettingsField(label: String, value: String, change: (String) -> Unit, secret: Boolean = false, enabled: Boolean = true,
    error: String? = null, detail: String? = null, singleLine: Boolean = true) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, fontSize = 12.sp, color = ZorkColors.Muted)
        OutlinedTextField(value, change, modifier = Modifier.fillMaxWidth().semantics { contentDescription = label }, enabled = enabled, singleLine = singleLine,
            isError = error != null, supportingText = (error ?: detail)?.let { message -> { Text(message, fontSize = 12.sp) } },
            shape = SettingsStyle.Field, visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None,
            textStyle = LocalTextStyle.current.copy(fontSize = 15.sp), colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = ZorkColors.Muted, unfocusedBorderColor = ZorkColors.FieldBorder,
                focusedContainerColor = ZorkColors.Canvas, unfocusedContainerColor = ZorkColors.Canvas))
    }
}
@Composable internal fun SettingsSelect(label: String, selected: String, options: List<Pair<String,String>>, enabled: Boolean = true, choose: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, fontSize = 12.sp, color = ZorkColors.Muted)
        Box {
            Surface(onClick = { expanded = true }, enabled = enabled && options.isNotEmpty(), modifier = Modifier.fillMaxWidth().semantics { contentDescription = label },
                shape = SettingsStyle.Field, border = BorderStroke(1.dp, ZorkColors.FieldBorder), color = ZorkColors.Canvas) {
                Row(Modifier.padding(horizontal = 14.dp).heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(options.find { it.first == selected }?.second ?: "请选择", modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 14.sp)
                    Glyph(R.drawable.ic_chevron_down, 16.dp, ZorkColors.Muted)
                }
            }
            DropdownMenu(expanded, { expanded = false }, modifier = Modifier.heightIn(max = 320.dp), shape = SettingsStyle.Card, containerColor = ZorkColors.Canvas) {
                options.forEach { (id,name) -> DropdownMenuItem(text = { Text(name, fontSize = 14.sp) }, onClick = { choose(id); expanded = false }, trailingIcon = { if (id == selected) Text("✓") }) }
            }
        }
    }
}
@Composable internal fun SettingsSegments(options: List<Pair<String,String>>, selected: String, enabled: Boolean = true, choose: (String) -> Unit) {
    Row(Modifier.fillMaxWidth().background(ZorkColors.Prompt, SettingsStyle.Pill).padding(4.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        options.forEach { (id,label) -> Surface(onClick = { choose(id) }, enabled = enabled, modifier = Modifier.weight(1f), shape = SettingsStyle.Pill,
            color = if (selected == id) ZorkColors.Canvas else ZorkColors.Prompt, shadowElevation = if (selected == id) 1.dp else 0.dp) {
            Box(Modifier.heightIn(min = 44.dp).padding(horizontal = 8.dp), contentAlignment = Alignment.Center) { Text(label, maxLines = 1, fontSize = 14.sp) }
        } }
    }
}
@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun SettingsSheet(title: String, busy: Boolean = false, error: String? = null, dismiss: () -> Unit,
    footer: (@Composable ColumnScope.() -> Unit)? = null, content: @Composable ColumnScope.() -> Unit) {
    val scroll=rememberScrollState()
    LaunchedEffect(error) { if(error!=null) scroll.animateScrollTo(0) }
    ModalBottomSheet(onDismissRequest = { if (!busy) dismiss() }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        dragHandle = null, containerColor = ZorkColors.Canvas, scrimColor = androidx.compose.ui.graphics.Color.Black.copy(alpha = .55f),
        shape = RoundedCornerShape(topStart = 32.dp, topEnd = 32.dp)) {
        Column(Modifier.fillMaxWidth().heightIn(max = (androidx.compose.ui.platform.LocalConfiguration.current.screenHeightDp * .86f).dp)
            .imePadding().then(if (footer == null) Modifier.verticalScroll(scroll) else Modifier).padding(24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(title, fontSize = 20.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                IconButton(onClick = dismiss, enabled = !busy, modifier = Modifier.size(44.dp)) { Icon(painterResource(R.drawable.ic_x), "关闭", Modifier.size(18.dp)) }
            }
            if (footer == null) {
                if (error != null) Text(error, color = ZorkColors.Danger, fontSize = 13.sp, modifier = Modifier.fillMaxWidth().background(ZorkColors.Prompt, SettingsStyle.Field).padding(12.dp))
                content()
            } else {
                Column(Modifier.weight(1f, fill = false).verticalScroll(scroll), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                    if (error != null) Text(error, color = ZorkColors.Danger, fontSize = 13.sp, modifier = Modifier.fillMaxWidth().background(ZorkColors.Prompt, SettingsStyle.Field).padding(12.dp))
                    content()
                }
                Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp), content = footer)
            }
        }
    }
}
@Composable internal fun ProviderMark(provider: String, size: Int = 24) {
    val id = when(provider) { "openai" -> R.drawable.provider_openai; "anthropic" -> R.drawable.provider_anthropic; "github-copilot" -> R.drawable.provider_githubcopilot; "kimi", "kimi-coding" -> R.drawable.provider_kimi; "openrouter" -> R.drawable.provider_openrouter; "xai" -> R.drawable.provider_xai; "opencode-go" -> R.drawable.provider_opencode; else -> R.drawable.provider_compatible }
    Image(painterResource(id), null, Modifier.size(size.dp))
}

@Composable
internal fun SettingsListGroup(content: @Composable ColumnScope.() -> Unit) {
    Surface(Modifier.fillMaxWidth(), shape = SettingsStyle.Card, color = ZorkColors.Paper) {
        Column(content = content)
    }
}

@Composable
internal fun SettingsListDivider() {
    HorizontalDivider(Modifier.padding(start = 60.dp, end = 16.dp), thickness = 0.5.dp, color = ZorkColors.FieldBorder)
}

@Composable
internal fun SettingsListRow(
    label: String,
    icon: Int? = null,
    avatar: String? = null,
    subtext: String? = null,
    detail: String? = null,
    value: String? = null,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
    action: (() -> Unit)? = null,
) {
    val interaction = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
        .background(if (pressed) ZorkColors.Selected else androidx.compose.ui.graphics.Color.Transparent)
        .clickable(interactionSource = interaction, indication = null, enabled = action != null,
            role = androidx.compose.ui.semantics.Role.Button, onClick = { action?.invoke() })
        .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        if (leading != null || avatar != null || icon != null) Box(Modifier.size(32.dp), contentAlignment = Alignment.Center) {
            when {
                leading != null -> leading()
                avatar != null -> Avatar(avatar, 32.dp)
                icon != null -> Icon(painterResource(icon), null, Modifier.size(24.dp), tint = ZorkColors.Muted)
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(label, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            subtext?.let { Text(it, fontSize = 11.sp, color = ZorkColors.Muted, maxLines = 2, overflow = TextOverflow.Ellipsis) }
            detail?.let { Text(it, fontSize = 11.sp, color = ZorkColors.Muted, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        value?.let { Text(it, fontSize = 12.sp, color = ZorkColors.Muted) }
        trailing?.invoke()
    }
}

@Composable
internal fun SettingsToggle(label: String, checked: Boolean, enabled: Boolean = true, detail: String? = null, change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(label, fontSize = 14.sp)
            detail?.let { Text(it, fontSize = 12.sp, color = ZorkColors.Muted) }
        }
        Switch(checked, change, enabled = enabled, modifier = Modifier.semantics { contentDescription = label })
    }
}

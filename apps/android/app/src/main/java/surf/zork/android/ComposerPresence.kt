package surf.zork.android

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.drawText
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import org.json.JSONObject
import kotlin.math.*

internal data class ComposerMember(val id: String, val name: String, val avatar: String, val label: String, val failed: Boolean)
private const val WIDTH_SPEED = 1440f // dp/s, matching desktop logical pixels/s
private const val LABEL_GAP = 8f
private const val LABEL_RIGHT_PADDING = 12f
private const val PORTRAIT_INSET = 4.8f
internal data class ComposerBubble(val member: ComposerMember, val x: Float, val lift: Float, val width: Float, val reveal: Float)
internal data class ComposerPresence(val bubbles: List<ComposerBubble>) {
    val extent: Float get() = bubbles.maxOfOrNull { it.lift + 16f }?.coerceAtLeast(0f) ?: 0f
}

@Stable
internal class ComposerMotion(val members: List<ComposerAnimatedMember>) {
    fun sample() = ComposerPresence(members.map { it.sample() })
    val extent: Float get() = members.maxOfOrNull { it.lift.value + 16f }?.coerceAtLeast(0f) ?: 0f
    fun extentPixels(density: Float) = (extent * density).roundToInt()
    val targetExtent: Float get() = members.maxOfOrNull { it.targetLift + 16f }?.coerceAtLeast(0f) ?: 0f
}
internal data class ComposerAnimatedMember(
    val member: ComposerMember, val x: State<Float>, val lift: State<Float>,
    val progress: State<Float>, val labelWidth: State<Float>, val labelLayout: TextLayoutResult, val targetLift: Float,
) {
    val reveal: Float get() = ((progress.value - .45f) / .55f).coerceIn(0f, 1f)
    fun sample() = ComposerBubble(member, x.value, lift.value, 32f + labelWidth.value, reveal)
}

internal fun activityLabel(activity: JSONObject?): String = when (activity?.text("state")) {
    "live" -> activity.optJSONObject("presentation")?.text("label_zh").orEmpty()
    "thinking", "tool_finished" -> "正在思考"
    "tools_started", "tools_waiting" -> if (activity.optBoolean("thinking") && activity.optJSONArray("calls").objects().isNotEmpty()) {
        val calls = activity.optJSONArray("calls").objects()
        val label = "正在思考"
        "$label · ${calls.size} 项操作执行中"
    } else activity.optJSONArray("calls").objects().joinToString(" · ") {
        if (it.text("action").isNotBlank()) return@joinToString it.text("action")
        val labels = it.optJSONObject("labels")
        val action = labels?.optString("zh-CN").orEmpty().ifBlank {
            labels?.optString("en").orEmpty().ifBlank { "执行操作" }
        }
        listOf(action, it.text("detail")).filter(String::isNotBlank).joinToString(" ")
    }.ifBlank { "正在处理" }
    "waiting" -> listOf("等待中", activity.text("reason")).filter(String::isNotBlank).joinToString(" · ")
    "failed" -> listOf("执行失败", activity.text("reason")).filter(String::isNotBlank).joinToString(" · ")
    else -> ""
}

@Composable
internal fun rememberComposerPresence(state: WorkbenchState, availableWidth: Float): ComposerMotion {
    val members = state.participants.map {
        val activity = it.optJSONObject("activity")
        ComposerMember(it.text("id"), it.text("name"), it.text("avatar"), activityLabel(activity), activity?.text("state") == "failed")
    }.ifEmpty {
        state.conversation?.let { listOf(ComposerMember(it.id, it.title, it.avatar.orEmpty(), state.activity, false)) }.orEmpty()
    }
    // Snapshot event order must not shuffle the dock while a member is moving.
    val order = remember(state.conversation?.id) { mutableListOf<String>() }
    order.retainAll(members.map { it.id }.toSet())
    members.forEach { if (it.id !in order) order.add(it.id) }
    val measure = rememberTextMeasurer(cacheSize = 64)
    val density = androidx.compose.ui.platform.LocalDensity.current
    var idle = 0
    var active = 0
    val bubbles = order.mapNotNull { id -> members.find { it.id == id } }.map { member -> key(member.id) {
        var mounted by remember { mutableStateOf(false) }
        LaunchedEffect(Unit) { mounted = true }
        var graceExpanded by remember { mutableStateOf(false) }
        val live = member.label.isNotBlank()
        // An active status retargets all members in this composition. Waiting
        // for one effect per member previously caused a second full UI update.
        val expanded = mounted && (live || graceExpanded)
        LaunchedEffect(live) {
            if (live) graceExpanded = true
            else { delay(1200); graceExpanded = false }
        }
        val label = member.label.ifBlank { "空闲" }
        val maxTextWidth = ((availableWidth - 32f - LABEL_GAP - LABEL_RIGHT_PADDING + PORTRAIT_INSET)
            .coerceAtLeast(0f) * density.density).toInt()
        val labelLayout = measure.measure("${member.name} · $label", TextStyle(fontFamily = ZorkFonts.Body, fontSize = 12.sp),
            maxLines = 1, overflow = TextOverflow.Ellipsis,
            constraints = androidx.compose.ui.unit.Constraints(maxWidth = maxTextWidth))
        val textWidth = with(density) { labelLayout.size.width.toDp().value }
        val targetX = if (expanded) 0f else 22f + idle++ * 22f
        val targetLift = if (expanded) 19f + active++ * 35f else -8f
        // Critically damped, velocity-preserving retargets; Compose respects the
        // platform animation scale and stops requesting frames after settling.
        val motion = spring<Float>(dampingRatio = 1f, stiffness = 406f, visibilityThreshold = .01f)
        val x = animateFloatAsState(targetX, motion, label = "member-x")
        val lift = animateFloatAsState(targetLift, motion, label = "member-lift")
        val progress = animateFloatAsState(if (expanded) 1f else 0f, motion, label = "member-label")
        val labelWidth = remember { Animatable(0f) }
        val targetWidth = if (expanded) (textWidth + LABEL_GAP + LABEL_RIGHT_PADDING - PORTRAIT_INSET)
            .coerceAtMost((availableWidth - 32f).coerceAtLeast(0f)) else 0f
        LaunchedEffect(targetWidth) {
            val duration = ceil(abs(targetWidth - labelWidth.value) / WIDTH_SPEED * 1000f).toInt()
            if (duration == 0) labelWidth.snapTo(targetWidth)
            else labelWidth.animateTo(targetWidth, tween(durationMillis = duration, easing = LinearEasing))
        }
        ComposerAnimatedMember(member.copy(label = label), x, lift, progress, labelWidth.asState(), labelLayout, targetLift)
    } }
    return remember(bubbles) { ComposerMotion(bubbles) }
}

@Composable
internal fun ComposerMembers(presence: ComposerMotion) {
    val ellipsisMeasurer = rememberTextMeasurer(cacheSize = 64)
    val labelDensity = androidx.compose.ui.platform.LocalDensity.current
    // Sample animation in layout/draw, never in composition. Text is measured
    // at its settled width; the reveal only changes the clipping boundary.
    androidx.compose.ui.layout.Layout(modifier = Modifier.fillMaxWidth(), content = {
        presence.members.forEach { animated -> key(animated.member.id) {
            Row(Modifier.drawWithContent {
                val revealedWidth = (22.4f + animated.labelWidth.value) * density
                clipRect(right = min(size.width, revealedWidth)) { this@drawWithContent.drawContent() }
            }, verticalAlignment = Alignment.CenterVertically) {
                ComposerPortrait(animated.member.avatar, animated.member.name)
                Spacer(Modifier.width(LABEL_GAP.dp))
                val label = "${animated.member.name} · ${animated.member.label}"
                Canvas(Modifier.width(with(labelDensity) { animated.labelLayout.size.width.toDp() }).fillMaxHeight()
                    .semantics { contentDescription = label }) {
                    val layout = if (animated.labelLayout.size.width <= size.width) animated.labelLayout else
                        ellipsisMeasurer.measure(label, animated.labelLayout.layoutInput.style,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                            constraints = androidx.compose.ui.unit.Constraints(maxWidth = size.width.toInt().coerceAtLeast(1)))
                    val visibleWidth = ((animated.labelWidth.value - LABEL_GAP - LABEL_RIGHT_PADDING + PORTRAIT_INSET)
                        .coerceAtLeast(0f) * density).coerceAtMost(size.width)
                    clipRect(right = visibleWidth) {
                        drawText(layout, color = if (animated.member.failed) ZorkColors.Danger else ZorkColors.Ink,
                            topLeft = androidx.compose.ui.geometry.Offset(0f, (size.height - layout.size.height) * .5f), alpha = animated.reveal)
                    }
                }
            }
        } }
    }) { measurables, constraints ->
        val extent = presence.extentPixels(density)
        val placeables = measurables.map { measurable ->
            val width = (constraints.maxWidth - 9.6.dp.roundToPx()).coerceAtLeast(1)
            measurable.measure(androidx.compose.ui.unit.Constraints.fixed(width, 22.4.dp.roundToPx()))
        }
        layout(constraints.maxWidth, extent + 12.dp.roundToPx()) {
            placeables.forEachIndexed { i, placeable ->
                val member = presence.members[i]
                placeable.place(((member.x.value + 4.8f) * density).roundToInt(), extent - ((member.lift.value + 11.2f) * density).roundToInt())
            }
        }
    }
}

@Composable
private fun ComposerPortrait(avatar: String, name: String) {
    val resource = when (avatar) {
        "fox" -> R.drawable.portrait_fox; "panda" -> R.drawable.portrait_panda
        "bear" -> R.drawable.portrait_bear; "bunny" -> R.drawable.portrait_bunny
        "chick" -> R.drawable.portrait_chick; "deer" -> R.drawable.portrait_deer
        "dog" -> R.drawable.portrait_dog; "koala" -> R.drawable.portrait_koala
        "octopus" -> R.drawable.portrait_octopus; "owl" -> R.drawable.portrait_owl
        "penguin" -> R.drawable.portrait_penguin; else -> R.drawable.portrait_cat
    }
    androidx.compose.foundation.Image(androidx.compose.ui.res.painterResource(resource), name, Modifier.size(22.4.dp))
}

// Same compact signed-distance union as the desktop composer. Only cells along
// the boundary are sampled; one cached, closed path supplies both fill and stroke.
internal fun Modifier.liquidComposer(presence: ComposerMotion, cache: ComposerContourCache): Modifier = drawWithCache {
    val factor = density
    val width = size.width / factor
    onDrawBehind {
        val frame = presence.sample()
        val top = frame.extent
        val height = size.height / factor - top
        val bubbles = frame.bubbles.map { it.copy(width = min(it.width, (width - it.x).coerceAtLeast(32f))) }
        val brush = cache.brush(width, height, top, factor, bubbles)
        if (brush != null) {
            val pad = factor * 2f
            drawRect(brush, topLeft = androidx.compose.ui.geometry.Offset(-pad, -pad),
                size = androidx.compose.ui.geometry.Size(size.width + pad * 2f, size.height + pad * 2f))
        } else {
            val path = cache.path(width, height, bubbles)
            scale(factor, factor, pivot = androidx.compose.ui.geometry.Offset.Zero) {
                withTransform({ translate(0f, top) }) {
                    drawPath(path, ZorkColors.Paper)
                    drawPath(path, ZorkColors.FieldBorder, style = Stroke(1f))
                }
            }
        }
    }
}

// Text/cursor/snapshot recompositions do not change the silhouette. Keep the
// geometry cache outside drawWithCache's lambda lifetime so those redraws reuse it.
internal class ComposerContourCache {
    private var width = -1f
    private var height = -1f
    private var geometry = emptyList<Triple<Float, Float, Float>>()
    private var cached = Path()
    private val tracer = ComposerContourTracer()
    private val gpu = if (android.os.Build.VERSION.SDK_INT >= 33) runCatching { ComposerGpuSurface() }.getOrNull() else null
    internal val gpuAvailable: Boolean get() = gpu != null
    fun brush(width: Float, height: Float, top: Float, density: Float, bubbles: List<ComposerBubble>): androidx.compose.ui.graphics.Brush? =
        if (android.os.Build.VERSION.SDK_INT >= 33) gpu?.brush(width, height, top, density, bubbles) else null
    fun path(width: Float, height: Float, bubbles: List<ComposerBubble>): Path {
        val geometry = bubbles.map { Triple(it.x, it.lift, it.width) }
        if (this.width != width || this.height != height || this.geometry != geometry) {
            cached = tracer.build(width, height, bubbles)
            this.width = width; this.height = height; this.geometry = geometry
        }
        return cached
    }
}

// Coordinates are bounded UI dimensions; the overflow-safe general-purpose
// hypot implementation adds substantial cost at every signed-distance sample.
private fun distance(x: Float, y: Float): Float = sqrt(x * x + y * y)

internal fun composerContour(width: Float, height: Float, bubbles: List<ComposerBubble>): Path =
    ComposerContourTracer().build(width, height, bubbles)

// Primitive buffers survive animation frames. Generation stamps invalidate the
// changing grid without clearing or allocating its entire area on every frame.
private class ComposerContourTracer {
    private var generation = 0
    private var values = FloatArray(0)
    private var sampled = IntArray(0)
    private var visited = IntArray(0)
    private var vertexStamp = IntArray(0)
    private var vertexIndex = IntArray(0)
    private var queue = IntArray(4096)
    private var pointX = FloatArray(4096)
    private var pointY = FloatArray(4096)
    private var edgeA = IntArray(4096)
    private var edgeB = IntArray(4096)
    private var traced = IntArray(4096)

    fun build(width: Float, height: Float, bubbles: List<ComposerBubble>): Path {
        val left = -8f
        val top = floor(bubbles.minOfOrNull { -it.lift - 16f }?.coerceAtMost(0f) ?: 0f) - 8f
        val cols = ceil(width + 16f).toInt() + 1
        val rows = ceil(height + 8f - top).toInt() + 1
        val cells = cols * rows
        if (values.size < cells) {
            val capacity = max(cells, values.size * 2)
            values = FloatArray(capacity); sampled = IntArray(capacity); visited = IntArray(capacity)
            vertexStamp = IntArray(capacity * 2); vertexIndex = IntArray(capacity * 2)
        }
        generation++
        if (generation == 0) { sampled.fill(0); visited.fill(0); vertexStamp.fill(0); traced.fill(0); generation = 1 }
        val stamp = generation
        fun union(a: Float, b: Float): Float {
            val h = ((4f - abs(a - b)) / 4f).coerceAtLeast(0f)
            return min(a, b) - h * h
        }
        fun plate(x: Float, y: Float): Float {
            if (x >= 24f && x <= width - 24f) return -min(y, 24f)
            val cx = x.coerceIn(24f, (width - 24f).coerceAtLeast(24f))
            return distance(x - cx, y - max(y, 24f)) - 24f
        }
        fun sample(i: Int): Float {
            if (sampled[i] == stamp) return values[i]
            val x = left + i % cols; val y = top + i / cols
            var actors = Float.POSITIVE_INFINITY
            for (index in bubbles.indices) {
                val b = bubbles[index]
                val start = b.x + 16f
                val center = x.coerceIn(start, (b.x + b.width - 16f).coerceAtLeast(start))
                actors = union(actors, distance(x - center, y + b.lift) - 16f)
            }
            val value = max(union(plate(x, y), actors), plate(x, height - y))
            sampled[i] = stamp; values[i] = value
            return value
        }
        var head = 0; var tail = 0
        fun enqueue(i: Int) {
            if (visited[i] == stamp) return
            visited[i] = stamp
            if (tail == queue.size) queue = queue.copyOf(queue.size * 2)
            queue[tail++] = i
        }
        for (source in -1 until bubbles.size) {
            val x = if (source < 0) width / 2f else bubbles[source].let { it.x + it.width / 2f }
            val col = (x - left).toInt().coerceIn(0, cols - 2)
            for (row in 0 until rows - 1) {
                val i = row * cols + col
                if ((sample(i) <= 0f) != (sample(i + cols) <= 0f)) enqueue(i)
            }
        }
        var pointCount = 0
        fun vertex(id: Int, a: Int, b: Int, va: Float, vb: Float): Int {
            if (vertexStamp[id] == stamp) return vertexIndex[id]
            val index = pointCount++
            if (index == pointX.size) {
                val capacity = pointX.size * 2
                pointX = pointX.copyOf(capacity); pointY = pointY.copyOf(capacity)
                edgeA = edgeA.copyOf(capacity); edgeB = edgeB.copyOf(capacity); traced = traced.copyOf(capacity)
            }
            val t = -va / (vb - va)
            pointX[index] = left + a % cols + ((b % cols) - (a % cols)) * t
            pointY[index] = top + a / cols + ((b / cols) - (a / cols)) * t
            edgeA[index] = -1; edgeB[index] = -1
            vertexStamp[id] = stamp; vertexIndex[id] = index
            return index
        }
        fun connect(a: Int, b: Int) {
            if (edgeA[a] == -1) edgeA[a] = b else edgeB[a] = b
            if (edgeA[b] == -1) edgeA[b] = a else edgeB[b] = a
        }
        // Scratch arrays are per contour build, not per grid cell.
        val corners = IntArray(4); val v = FloatArray(4); val crossings = IntArray(4)
        while (head < tail) {
            val i = queue[head++]; val row = i / cols; val col = i % cols
            corners[0] = i; corners[1] = i + 1; corners[2] = i + cols + 1; corners[3] = i + cols
            for (j in 0..3) v[j] = sample(corners[j])
            var count = 0
            for (edge in 0..3) {
                val next = (edge + 1) % 4
                if ((v[edge] <= 0f) == (v[next] <= 0f)) continue
                val id = when (edge) { 0 -> i * 2; 1 -> (i + 1) * 2 + 1; 2 -> (i + cols) * 2; else -> i * 2 + 1 }
                crossings[count++] = vertex(id, corners[edge], corners[next], v[edge], v[next])
                when (edge) {
                    0 -> if (row > 0) enqueue(i - cols)
                    1 -> if (col < cols - 2) enqueue(i + 1)
                    2 -> if (row < rows - 2) enqueue(i + cols)
                    3 -> if (col > 0) enqueue(i - 1)
                }
            }
            if (count == 2) connect(crossings[0], crossings[1])
            else if (count == 4) {
                if ((v.sum() <= 0f) == (v[0] <= 0f)) {
                    connect(crossings[0], crossings[1]); connect(crossings[2], crossings[3])
                } else { connect(crossings[0], crossings[3]); connect(crossings[1], crossings[2]) }
            }
        }
        val path = Path().apply { fillType = PathFillType.EvenOdd }
        for (start in 0 until pointCount) {
            if (traced[start] == stamp) continue
            var previous = -1; var current = start
            val contour = ArrayList<Pair<Float, Float>>()
            while (current >= 0 && traced[current] != stamp) {
                traced[current] = stamp; contour.add(Pair(pointX[current], pointY[current]))
                val next = if (edgeA[current] != previous) edgeA[current] else edgeB[current]
                previous = current; current = next
            }
            appendSmoothContour(path, contour)
        }
        return path
    }
}

// Match the desktop's subpixel RDP reduction and bounded cubic tangents. A
// thousand one-dp line segments along straight edges need not reach Skia.
private fun appendSmoothContour(path: Path, contour: List<Pair<Float, Float>>) {
    if (contour.size < 3) return
    val closed = contour + contour.first()
    val reduced = ArrayList<Pair<Float, Float>>()
    fun simplify(first: Int, last: Int) {
        if (first >= last) return
        val a = closed[first]; val b = closed[last]
        val dx = b.first - a.first; val dy = b.second - a.second
        val norm = distance(dx, dy).coerceAtLeast(.0001f)
        var farthest = first; var error = 0f
        for (i in first + 1 until last) {
            val p = closed[i]
            val d = abs((p.first - a.first) * dy - (p.second - a.second) * dx) / norm
            if (d > error) { error = d; farthest = i }
        }
        if (error > .12f) { simplify(first, farthest); simplify(farthest, last) }
        else reduced.add(a)
    }
    val half = contour.size / 2
    simplify(0, half); simplify(half, contour.size)
    fun length(a: Pair<Float, Float>, b: Pair<Float, Float>) = distance(b.first - a.first, b.second - a.second).coerceAtLeast(.0001f)
    fun tangent(a: Pair<Float, Float>, b: Pair<Float, Float>, c: Pair<Float, Float>): Pair<Float, Float> {
        val ab = length(a, b); val bc = length(b, c)
        val x = (b.first - a.first) / ab + (c.first - b.first) / bc
        val y = (b.second - a.second) / ab + (c.second - b.second) / bc
        val norm = distance(x, y).coerceAtLeast(.0001f)
        return Pair(x / norm, y / norm)
    }
    path.moveTo(reduced[0].first, reduced[0].second)
    val n = reduced.size
    for (i in 0 until n) {
        val previous = reduced[(i + n - 1) % n]; val a = reduced[i]
        val b = reduced[(i + 1) % n]; val after = reduced[(i + 2) % n]
        val span = length(a, b) / 3f
        val ta = tangent(previous, a, b); val tb = tangent(a, b, after)
        val da = min(span, length(previous, a) / 3f); val db = min(span, length(b, after) / 3f)
        path.cubicTo(a.first + ta.first * da, a.second + ta.second * da,
            b.first - tb.first * db, b.second - tb.second * db, b.first, b.second)
    }
    path.close()
}

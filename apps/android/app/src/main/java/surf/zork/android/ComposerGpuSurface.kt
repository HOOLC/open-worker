package surf.zork.android

import android.graphics.RuntimeShader
import androidx.annotation.RequiresApi
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.toArgb

/** One direct GPU draw, with no bitmap mask, blur, or offscreen text layer. */
@RequiresApi(33)
internal class ComposerGpuSurface {
    private val shader = RuntimeShader(SOURCE).apply {
        setColorUniform("surfaceColor", ZorkColors.Paper.toArgb())
        setColorUniform("borderColor", ZorkColors.FieldBorder.toArgb())
    }
    private val brush = ShaderBrush(shader)
    private val actors = FloatArray(16 * 4)
    fun brush(width: Float, height: Float, top: Float, density: Float, bubbles: List<ComposerBubble>): Brush? {
        if (bubbles.size > 16) return null
        for (i in bubbles.indices) {
            val b = bubbles[i]
            actors[i * 4] = b.x; actors[i * 4 + 1] = b.lift; actors[i * 4 + 2] = b.width
        }
        shader.setFloatUniform("geometry", width, height, top, density)
        shader.setFloatUniform("actors", actors)
        shader.setIntUniform("actorCount", bubbles.size)
        return brush
    }
    companion object {
        // Distance and its analytic gradient travel together. Normalizing by
        // the gradient keeps the border width even through a liquid neck.
        private const val SOURCE = """
            uniform float4 geometry;
            uniform float4 actors[16];
            uniform int actorCount;
            layout(color) uniform half4 surfaceColor;
            layout(color) uniform half4 borderColor;
            float3 smoothUnion(float3 a, float3 b) {
                float h = max((4.0 - abs(a.x - b.x)) * 0.25, 0.0);
                float wa = a.x < b.x ? 1.0 - h * 0.5 : h * 0.5;
                return float3(min(a.x, b.x) - h * h, mix(b.yz, a.yz, wa));
            }
            float3 plate(float2 p) {
                if (p.x >= 24.0 && p.x <= geometry.x - 24.0) {
                    return float3(-min(p.y, 24.0), 0.0, p.y < 24.0 ? -1.0 : 0.0);
                }
                float2 v = p - float2(clamp(p.x, 24.0, max(geometry.x - 24.0, 24.0)), max(p.y, 24.0));
                float len = length(v);
                return float3(len - 24.0, v / max(len, 0.0001));
            }
            half4 main(float2 coordinate) {
                float2 p = coordinate / geometry.w - float2(0.0, geometry.z);
                float3 members = float3(1000000.0, 0.0, 0.0);
                for (int i = 0; i < 16; ++i) {
                    if (i >= actorCount) break;
                    float4 b = actors[i];
                    float center = clamp(p.x, b.x + 16.0, max(b.x + b.z - 16.0, b.x + 16.0));
                    float2 v = float2(p.x - center, p.y + b.y);
                    float len = length(v);
                    members = smoothUnion(members, float3(len - 16.0, v / max(len, 0.0001)));
                }
                float3 field = smoothUnion(plate(p), members);
                float3 bottom = plate(float2(p.x, geometry.y - p.y));
                bottom.z = -bottom.z;
                if (bottom.x > field.x) field = bottom;
                float d = field.x / max(length(field.yz), 0.01);
                float coverage = clamp(0.5 - (d - 0.5) * geometry.w, 0.0, 1.0);
                float border = clamp(0.5 + (d + 0.5) * geometry.w, 0.0, 1.0);
                return mix(surfaceColor, borderColor, half(border)) * half(coverage);
            }
        """
    }
}

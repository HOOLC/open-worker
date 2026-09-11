#include <metal_stdlib>
using namespace metal;
struct Parameters {
    float width, height, scale, plate_y;
    float plate_height, radius, member_fusion, dock_fusion;
    float4 surface, lower;
    uint count, pixel_width, pixel_height;
    float surface_radius;
};
float blend_distance(float a, float b, float k) {
    float h=max(k-abs(a-b),0.0f)/k;
    return min(a,b)-h*h*k*0.25f;
}
float rounded_box(float2 p, float2 center, float2 half_size, float radius) {
    float2 q=abs(p-center)-(half_size-radius);
    return length(max(q,0.0f))+min(max(q.x,q.y),0.0f)-radius;
}
kernel void liquid_field(device uchar4 *output [[buffer(0)]],
                         constant Parameters &s [[buffer(1)]],
                         device const float4 *bubbles [[buffer(2)]],
                         uint2 id [[thread_position_in_grid]]) {
    if(id.x>=s.pixel_width || id.y>=s.pixel_height) return;
    float2 p=(float2(id)+0.5f)/s.scale-2.0f;
    float plate=rounded_box(p,float2(s.width*0.5f,s.plate_y+s.plate_height*0.5f),
                              float2(s.width*0.5f,s.plate_height*0.5f),s.surface_radius);
    float actors=1e10f;
    for(uint i=0;i<s.count;i++) {
        float4 b=bubbles[i];
        float cx=clamp(p.x,b.x+s.radius,max(b.x+s.radius,b.x+b.z-s.radius));
        float d=length(float2(p.x-cx,p.y-(s.plate_y-b.y)))-s.radius;
        actors=blend_distance(actors,d,s.member_fusion);
    }
    float d=blend_distance(plate,actors,s.dock_fusion);
    float alpha=clamp(0.5f-d*s.scale,0.0f,1.0f);
    float gradient=clamp((p.y-s.plate_y-s.plate_height*0.5f)/(s.plate_height*0.5f),0.0f,1.0f);
    float3 rgb=mix(s.surface.rgb,s.lower.rgb,gradient);
    // GPUI's image atlas consumes unpremultiplied BGRA bytes.
    output[id.y*s.pixel_width+id.x]=uchar4(round(float4(rgb.bgr,alpha)*255.0f));
}

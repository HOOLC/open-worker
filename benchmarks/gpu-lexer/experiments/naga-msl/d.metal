#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size1;
    uint size2;
};

struct ve {
    uint a;
    uint b;
    uint c;
    uint d;
};
struct we {
    uint start;
    uint count;
    uint e;
    uint c;
    uint f;
    uint g;
};
typedef we type_1[1];
typedef float type_3[1];
struct type_5 {
    metal::float4 inner[1024];
};

uint X(
    metal::uint3 Yb
) {
    return Yb.x + (Yb.y * 65535u);
}

struct dInput {
};
kernel void d(
  metal::uint3 fe [[threadgroup_position_in_grid]]
, metal::uint3 I [[thread_position_in_threadgroup]]
, constant ve& F [[buffer(0)]]
, device type_1 const& N [[buffer(1)]]
, device type_3& l [[buffer(2)]]
, threadgroup type_5& j
, constant _mslBufferSizes& _buffer_sizes [[buffer(3)]]
) {
    if (metal::all(I == metal::uint3(0u))) {
        j = {};
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    float xb = 0.0;
    float yb = 0.0;
    uint zb = 0u;
    metal::float4 x = {};
    uint ga = {};
    metal::float4 ha = {};
    metal::float4 ia = {};
    uint _e2 = X(fe);
    uint Hc = _e2 >> 2u;
    uint _e7 = F.a;
    if (Hc >= _e7) {
        return;
    }
    we fa = N[metal::min(unsigned(Hc), (_buffer_sizes.size1 - 0 - 24) / 24)];
    uint wb = I.x & 31u;
    uint Ic = ((_e2 & 3u) * 8u) + (I.x >> 5u);
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e188 = zb;
            zb = _e188 + 32u;
        }
        loop_init = false;
        uint _e29 = zb;
        if (_e29 < fa.c) {
        } else {
            break;
        }
        {
            uint _e32 = zb;
            uint Ab = _e32 + wb;
            uint ge = (fa.c - 1u) - Ab;
            bool Jc = Ab < fa.c;
            uint Bb = (((fa.e + Ab) * 32u) + Ic) * 4u;
            uint Cb = (((fa.e + ge) * 32u) + Ic) * 4u;
            x = metal::float4(1.0, 0.0, 1.0, 0.0);
            if (Jc) {
                float _e62 = l[metal::min(unsigned(Bb), (_buffer_sizes.size2 - 0 - 4) / 4)];
                float _e67 = l[metal::min(unsigned(Bb + 1u), (_buffer_sizes.size2 - 0 - 4) / 4)];
                float _e72 = l[metal::min(unsigned(Cb + 2u), (_buffer_sizes.size2 - 0 - 4) / 4)];
                float _e77 = l[metal::min(unsigned(Cb + 3u), (_buffer_sizes.size2 - 0 - 4) / 4)];
                x = metal::float4(_e62, _e67, _e72, _e77);
            }
            metal::float4 _e82 = x;
            j.inner[metal::min(unsigned(I.x), 1023u)] = _e82;
            ga = 1u;
            uint2 loop_bound_1 = uint2(4294967295u);
            bool loop_init_1 = true;
            while(true) {
                if (metal::all(loop_bound_1 == uint2(0u))) { break; }
                loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
                if (!loop_init_1) {
                    uint _e133 = ga;
                    ga = _e133 << 1u;
                }
                loop_init_1 = false;
                uint _e85 = ga;
                if (_e85 < 32u) {
                } else {
                    break;
                }
                {
                    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
                    ha = metal::float4(1.0, 0.0, 1.0, 0.0);
                    uint _e94 = ga;
                    if (wb >= _e94) {
                        uint _e98 = ga;
                        metal::float4 _e101 = j.inner[metal::min(unsigned(I.x - _e98), 1023u)];
                        ha = _e101;
                    }
                    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
                    float _e103 = x.x;
                    float _e105 = ha.x;
                    float _e108 = x.x;
                    float _e110 = ha.y;
                    float _e113 = x.y;
                    float _e116 = x.z;
                    float _e118 = ha.z;
                    float _e121 = x.z;
                    float _e123 = ha.w;
                    float _e126 = x.w;
                    x = metal::float4(_e103 * _e105, (_e108 * _e110) + _e113, _e116 * _e118, (_e121 * _e123) + _e126);
                    metal::float4 _e132 = x;
                    j.inner[metal::min(unsigned(I.x), 1023u)] = _e132;
                }
            }
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
            ia = metal::float4(1.0, 0.0, 1.0, 0.0);
            if (wb > 0u) {
                metal::float4 _e149 = j.inner[metal::min(unsigned(I.x - 1u), 1023u)];
                ia = _e149;
            }
            if (Jc) {
                float _e155 = ia.x;
                float _e156 = xb;
                float _e159 = ia.y;
                l[metal::min(unsigned(Bb + 1u), (_buffer_sizes.size2 - 0 - 4) / 4)] = (_e155 * _e156) + _e159;
                float _e166 = ia.z;
                float _e167 = yb;
                float _e170 = ia.w;
                l[metal::min(unsigned(Cb + 3u), (_buffer_sizes.size2 - 0 - 4) / 4)] = (_e166 * _e167) + _e170;
            }
            metal::float4 Ga = j.inner[metal::min(unsigned(I.x | 31u), 1023u)];
            float _e179 = xb;
            xb = (Ga.x * _e179) + Ga.y;
            float _e184 = yb;
            yb = (Ga.z * _e184) + Ga.w;
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        }
    }
    return;
}

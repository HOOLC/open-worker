#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size0;
    uint size2;
    uint size3;
    uint size4;
    uint size5;
    uint size6;
    uint size7;
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
struct xe {
    uint start;
    uint count;
    uint stream;
    uint h;
};
typedef uint type_1[1];
typedef we type_2[1];
typedef float type_4[1];
typedef xe type_5[1];
typedef metal::uint2 type_7[1];
struct type_9 {
    metal::float4 inner[1024];
};

uint X(
    metal::uint3 Yb
) {
    return Yb.x + (Yb.y * 65535u);
}

float fb(
    float Gd
) {
    return 1.0 / (1.0 + metal::exp(-(Gd)));
}

float gb(
    uint wa,
    uint Hd,
    uint Id,
    uint t,
    device type_1 const& M,
    device type_4 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    bool local_2 = {};
    float u = {};
    uint xa = 0u;
    if (!((wa < Hd))) {
        local_2 = wa >= Id;
    } else {
        local_2 = true;
    }
    bool _e10 = local_2;
    if (_e10) {
        return 0.0;
    }
    uint Y = M[metal::min(unsigned(wa * 2u), (_buffer_sizes.size0 - 0 - 4) / 4)];
    uint O = M[metal::min(unsigned((wa * 2u) + 1u), (_buffer_sizes.size0 - 0 - 4) / 4)];
    uint ac = Y & 3u;
    float _e33 = i[metal::min(unsigned((0u + (ac * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
    u = _e33;
    float _e35 = u;
    float _e49 = i[metal::min(unsigned((0u + ((4u + ((Y >> 2u) & 7u)) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
    u = _e35 + _e49;
    float _e51 = u;
    float _e65 = i[metal::min(unsigned((0u + ((12u + ((Y >> 5u) & 127u)) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
    u = _e51 + _e65;
    float _e67 = u;
    float _e81 = i[metal::min(unsigned((0u + ((140u + ((Y >> 12u) & 127u)) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
    u = _e67 + _e81;
    if (ac == 0u) {
        float _e85 = u;
        float _e101 = i[metal::min(unsigned((0u + ((268u + (((Y >> 19u) & 255u) & 255u)) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e85 + _e101;
        float _e103 = u;
        float _e117 = i[metal::min(unsigned((0u + ((524u + ((O >> 15u) & 127u)) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e103 + _e117;
    }
    uint Kd = O & 127u;
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e146 = xa;
            xa = _e146 + 1u;
        }
        loop_init = false;
        uint _e124 = xa;
        if (_e124 < 7u) {
        } else {
            break;
        }
        {
            uint _e128 = xa;
            if ((Kd & (1u << _e128)) != 0u) {
                float _e133 = u;
                uint _e136 = xa;
                float _e143 = i[metal::min(unsigned((0u + ((652u + _e136) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
                u = _e133 + _e143;
            }
        }
    }
    uint bc = (O >> 7u) & 15u;
    uint cc = (O >> 11u) & 15u;
    if (bc != 0u) {
        float _e158 = u;
        float _e170 = i[metal::min(unsigned((0u + (((659u + bc) - 1u) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e158 + _e170;
    }
    if (cc != 0u) {
        float _e174 = u;
        float _e186 = i[metal::min(unsigned((0u + (((673u + cc) - 1u) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e174 + _e186;
    }
    uint dc = (O >> 22u) & 31u;
    uint ec = (O >> 27u) & 31u;
    if (dc != 0u) {
        float _e198 = u;
        float _e210 = i[metal::min(unsigned((0u + (((687u + dc) - 1u) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e198 + _e210;
    }
    if (ec != 0u) {
        float _e214 = u;
        float _e226 = i[metal::min(unsigned((0u + (((718u + ec) - 1u) * 32u)) + t), (_buffer_sizes.size3 - 0 - 4) / 4)];
        u = _e214 + _e226;
    }
    float _e228 = u;
    return _e228;
}

float Ed(
    uint kb,
    uint H,
    uint ya,
    uint za,
    metal::float2 fc,
    device type_1 const& M,
    device type_4 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    float Aa = {};
    uint P = 0u;
    bool local_3 = {};
    float _e9 = i[metal::min(unsigned(23968u + H), (_buffer_sizes.size3 - 0 - 4) / 4)];
    Aa = _e9;
    uint2 loop_bound_1 = uint2(4294967295u);
    bool loop_init_1 = true;
    while(true) {
        if (metal::all(loop_bound_1 == uint2(0u))) { break; }
        loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
        if (!loop_init_1) {
            uint _e48 = P;
            P = _e48 + 1u;
        }
        loop_init_1 = false;
        uint _e13 = P;
        if (_e13 < 5u) {
        } else {
            break;
        }
        {
            uint _e16 = P;
            if ((kb + _e16) >= (ya + 2u)) {
                uint _e23 = P;
                local_3 = (kb + _e23) < (za + 2u);
            } else {
                local_3 = false;
            }
            bool _e29 = local_3;
            if (_e29) {
                float _e30 = Aa;
                uint _e31 = P;
                float _e35 = gb((kb + _e31) - 2u, ya, za, H, M, i, _buffer_sizes);
                uint _e38 = P;
                float _e44 = i[metal::min(unsigned((36905u + (_e38 * 32u)) + H), (_buffer_sizes.size3 - 0 - 4) / 4)];
                Aa = _e30 + (_e35 * _e44);
            }
        }
    }
    float _e50 = Aa;
    float _e53 = gb(as_type<uint>(fc.x), ya, za, H, M, i, _buffer_sizes);
    float _e58 = i[metal::min(unsigned(37065u + H), (_buffer_sizes.size3 - 0 - 4) / 4)];
    Aa = _e50 + (_e53 * _e58);
    float _e61 = Aa;
    float _e64 = gb(as_type<uint>(fc.y), ya, za, H, M, i, _buffer_sizes);
    float _e69 = i[metal::min(unsigned(37097u + H), (_buffer_sizes.size3 - 0 - 4) / 4)];
    Aa = _e61 + (_e64 * _e69);
    float _e72 = Aa;
    return metal::tanh(_e72);
}

metal::float2 Vb(
    uint Ld,
    uint Ba,
    device type_4 const& i,
    threadgroup type_9& j,
    constant _mslBufferSizes& _buffer_sizes
) {
    float gc = {};
    float hc = {};
    uint Z = 0u;
    float _e6 = i[metal::min(unsigned(38153u + Ba), (_buffer_sizes.size3 - 0 - 4) / 4)];
    gc = _e6;
    float _e12 = i[metal::min(unsigned(39209u + Ba), (_buffer_sizes.size3 - 0 - 4) / 4)];
    hc = _e12;
    uint2 loop_bound_2 = uint2(4294967295u);
    bool loop_init_2 = true;
    while(true) {
        if (metal::all(loop_bound_2 == uint2(0u))) { break; }
        loop_bound_2 -= uint2(loop_bound_2.y == 0u, 1u);
        if (!loop_init_2) {
            uint _e52 = Z;
            Z = _e52 + 1u;
        }
        loop_init_2 = false;
        uint _e16 = Z;
        if (_e16 < 32u) {
        } else {
            break;
        }
        {
            uint _e22 = Z;
            float ic = j.inner[metal::min(unsigned((Ld * 32u) + _e22), 1023u)].x;
            float _e27 = gc;
            uint _e33 = Z;
            float _e36 = i[metal::min(unsigned((37129u + (Ba * 32u)) + _e33), (_buffer_sizes.size3 - 0 - 4) / 4)];
            gc = _e27 + (ic * _e36);
            float _e39 = hc;
            uint _e45 = Z;
            float _e48 = i[metal::min(unsigned((38185u + (Ba * 32u)) + _e45), (_buffer_sizes.size3 - 0 - 4) / 4)];
            hc = _e39 + (ic * _e48);
        }
    }
    float _e54 = hc;
    float _e55 = fb(_e54);
    float _e58 = gc;
    return metal::float2(_e55, (1.0 - _e55) * metal::tanh(_e58));
}

struct cInput {
};
kernel void c(
  metal::uint3 de [[threadgroup_position_in_grid]]
, metal::uint3 ee [[thread_position_in_threadgroup]]
, device type_1 const& M [[buffer(0)]]
, constant ve& F [[buffer(1)]]
, device type_2 const& N [[buffer(2)]]
, device type_4 const& i [[buffer(3)]]
, device type_5 const& va [[buffer(4)]]
, device type_4& ib [[buffer(5)]]
, device type_4& l [[buffer(6)]]
, device type_7 const& G [[buffer(7)]]
, threadgroup type_9& j
, constant _mslBufferSizes& _buffer_sizes [[buffer(8)]]
) {
    if (metal::all(ee == metal::uint3(0u))) {
        j = {};
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint Ac = {};
    uint Bc = {};
    uint Da = {};
    bool local = {};
    uint Ea = {};
    bool local_1 = {};
    uint Q = 0u;
    uint ca = 0u;
    metal::float2 da = metal::float2(1.0, 0.0);
    uint sb = 0u;
    metal::float2 ea = metal::float2(1.0, 0.0);
    uint ub = {};
    uint _e2 = X(de);
    uint _e5 = F.c;
    if (_e2 >= _e5) {
        return;
    }
    xe s = va[metal::min(unsigned(_e2), (_buffer_sizes.size4 - 0 - 16) / 16)];
    we rb = N[metal::min(unsigned(s.stream), (_buffer_sizes.size2 - 0 - 24) / 24)];
    uint q = ee.x;
    if (q < s.count) {
        uint zc = s.start + q;
        uint _e22 = G[metal::min(unsigned(_e2), (_buffer_sizes.size7 - 0 - 8) / 8)].y;
        Ac = _e22;
        uint _e27 = G[metal::min(unsigned(_e2), (_buffer_sizes.size7 - 0 - 8) / 8)].x;
        Bc = _e27;
        Da = zc;
        uint2 loop_bound_3 = uint2(4294967295u);
        bool loop_init_3 = true;
        while(true) {
            if (metal::all(loop_bound_3 == uint2(0u))) { break; }
            loop_bound_3 -= uint2(loop_bound_3.y == 0u, 1u);
            if (!loop_init_3) {
                uint _e55 = Da;
                Da = _e55 - 1u;
            }
            loop_init_3 = false;
            uint _e30 = Da;
            if (_e30 > s.start) {
            } else {
                break;
            }
            {
                uint _e34 = Da;
                uint _e40 = M[metal::min(unsigned((_e34 - 1u) * 2u), (_buffer_sizes.size0 - 0 - 4) / 4)];
                uint Cc = _e40 & 3u;
                if (Cc != 1u) {
                    local = Cc != 2u;
                } else {
                    local = false;
                }
                bool _e50 = local;
                if (_e50) {
                    uint _e51 = Da;
                    Ac = _e51 - 1u;
                    break;
                }
            }
        }
        Ea = zc + 1u;
        uint2 loop_bound_4 = uint2(4294967295u);
        bool loop_init_4 = true;
        while(true) {
            if (metal::all(loop_bound_4 == uint2(0u))) { break; }
            loop_bound_4 -= uint2(loop_bound_4.y == 0u, 1u);
            if (!loop_init_4) {
                uint _e83 = Ea;
                Ea = _e83 + 1u;
            }
            loop_init_4 = false;
            uint _e60 = Ea;
            if (_e60 < (s.start + s.count)) {
            } else {
                break;
            }
            {
                uint _e66 = Ea;
                uint _e70 = M[metal::min(unsigned(_e66 * 2u), (_buffer_sizes.size0 - 0 - 4) / 4)];
                uint Dc = _e70 & 3u;
                if (Dc != 1u) {
                    local_1 = Dc != 2u;
                } else {
                    local_1 = false;
                }
                bool _e80 = local_1;
                if (_e80) {
                    uint _e81 = Ea;
                    Bc = _e81;
                    break;
                }
            }
        }
        uint _e90 = Ac;
        j.inner[metal::min(unsigned(q * 32u), 1023u)].y = as_type<float>(_e90);
        uint _e97 = Bc;
        j.inner[metal::min(unsigned(q * 32u), 1023u)].z = as_type<float>(_e97);
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint2 loop_bound_5 = uint2(4294967295u);
    bool loop_init_5 = true;
    while(true) {
        if (metal::all(loop_bound_5 == uint2(0u))) { break; }
        loop_bound_5 -= uint2(loop_bound_5.y == 0u, 1u);
        if (!loop_init_5) {
            uint _e135 = Q;
            Q = _e135 + 1u;
        }
        loop_init_5 = false;
        uint _e101 = Q;
        if (_e101 < s.count) {
        } else {
            break;
        }
        {
            uint _e105 = Q;
            uint _e112 = Q;
            metal::float4 _e116 = j.inner[metal::min(unsigned(_e112 * 32u), 1023u)];
            float _e118 = Ed(s.start + _e105, q, rb.start, rb.start + rb.count, _e116.yz, M, i, _buffer_sizes);
            uint _e121 = Q;
            ib[metal::min(unsigned(((s.start + _e121) * 32u) + q), (_buffer_sizes.size5 - 0 - 4) / 4)] = _e118;
            uint _e128 = Q;
            j.inner[metal::min(unsigned((_e128 * 32u) + q), 1023u)].x = _e118;
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint2 loop_bound_6 = uint2(4294967295u);
    bool loop_init_6 = true;
    while(true) {
        if (metal::all(loop_bound_6 == uint2(0u))) { break; }
        loop_bound_6 -= uint2(loop_bound_6.y == 0u, 1u);
        if (!loop_init_6) {
            uint _e161 = ca;
            ca = _e161 + 1u;
        }
        loop_init_6 = false;
        uint _e139 = ca;
        if (_e139 < s.count) {
        } else {
            break;
        }
        {
            uint _e142 = ca;
            metal::float2 _e143 = Vb(_e142, q, i, j, _buffer_sizes);
            uint _e145 = ca;
            j.inner[metal::min(unsigned((_e145 * 32u) + q), 1023u)].y = _e143.x;
            uint _e153 = ca;
            j.inner[metal::min(unsigned((_e153 * 32u) + q), 1023u)].z = _e143.y;
        }
    }
    uint2 loop_bound_7 = uint2(4294967295u);
    bool loop_init_7 = true;
    while(true) {
        if (metal::all(loop_bound_7 == uint2(0u))) { break; }
        loop_bound_7 -= uint2(loop_bound_7.y == 0u, 1u);
        if (!loop_init_7) {
            uint _e191 = sb;
            sb = _e191 + 1u;
        }
        loop_init_7 = false;
        uint _e169 = sb;
        if (_e169 < s.count) {
        } else {
            break;
        }
        {
            uint _e173 = sb;
            metal::float4 tb = j.inner[metal::min(unsigned((_e173 * 32u) + q), 1023u)];
            float _e181 = da.x;
            float _e185 = da.y;
            da = metal::float2(tb.y * _e181, (tb.y * _e185) + tb.z);
        }
    }
    ub = s.count;
    uint2 loop_bound_8 = uint2(4294967295u);
    bool loop_init_8 = true;
    while(true) {
        if (metal::all(loop_bound_8 == uint2(0u))) { break; }
        loop_bound_8 -= uint2(loop_bound_8.y == 0u, 1u);
        if (!loop_init_8) {
            uint _e223 = ub;
            ub = _e223 - 1u;
        }
        loop_init_8 = false;
        uint _e199 = ub;
        if (_e199 > 0u) {
        } else {
            break;
        }
        {
            uint _e203 = ub;
            metal::float4 vb = j.inner[metal::min(unsigned(((_e203 - 1u) * 32u) + q), 1023u)];
            float _e213 = ea.x;
            float _e217 = ea.y;
            ea = metal::float2(vb.y * _e213, (vb.y * _e217) + vb.z);
        }
    }
    uint Fa = ((_e2 * 32u) + q) * 4u;
    float _e233 = da.x;
    l[metal::min(unsigned(Fa), (_buffer_sizes.size6 - 0 - 4) / 4)] = _e233;
    float _e239 = da.y;
    l[metal::min(unsigned(Fa + 1u), (_buffer_sizes.size6 - 0 - 4) / 4)] = _e239;
    float _e245 = ea.x;
    l[metal::min(unsigned(Fa + 2u), (_buffer_sizes.size6 - 0 - 4) / 4)] = _e245;
    float _e251 = ea.y;
    l[metal::min(unsigned(Fa + 3u), (_buffer_sizes.size6 - 0 - 4) / 4)] = _e251;
    return;
}

#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size1;
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
typedef we type_1[1];
typedef float type_3[1];
typedef half type_5[1];
typedef xe type_6[1];
struct type_8 {
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

metal::float2 Vb(
    uint Ld,
    uint Ba,
    device type_3 const& i,
    threadgroup type_8& j,
    constant _mslBufferSizes& _buffer_sizes
) {
    float gc = {};
    float hc = {};
    uint Z = 0u;
    float _e6 = i[metal::min(unsigned(38153u + Ba), (_buffer_sizes.size2 - 0 - 4) / 4)];
    gc = _e6;
    float _e12 = i[metal::min(unsigned(39209u + Ba), (_buffer_sizes.size2 - 0 - 4) / 4)];
    hc = _e12;
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e52 = Z;
            Z = _e52 + 1u;
        }
        loop_init = false;
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
            float _e36 = i[metal::min(unsigned((37129u + (Ba * 32u)) + _e33), (_buffer_sizes.size2 - 0 - 4) / 4)];
            gc = _e27 + (ic * _e36);
            float _e39 = hc;
            uint _e45 = Z;
            float _e48 = i[metal::min(unsigned((38185u + (Ba * 32u)) + _e45), (_buffer_sizes.size2 - 0 - 4) / 4)];
            hc = _e39 + (ic * _e48);
        }
    }
    float _e54 = hc;
    float _e55 = fb(_e54);
    float _e58 = gc;
    return metal::float2(_e55, (1.0 - _e55) * metal::tanh(_e58));
}

float hb(
    float kc,
    float lc,
    float Md,
    float Nd,
    uint mc,
    uint Od,
    device type_3 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    uint aa = (metal::min(Od, 11u) * 32u) + mc;
    float _e15 = i[metal::min(unsigned(24000u + aa), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e21 = i[metal::min(unsigned(24384u + aa), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e28 = i[metal::min(unsigned(24768u + aa), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e35 = i[metal::min(unsigned(25152u + aa), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e42 = i[metal::min(unsigned(25536u + aa), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float Pd = metal::tanh(((((kc * _e15) + (lc * _e21)) + (Md * _e28)) + (Nd * _e35)) + _e42);
    float Qd = (mc < 16u) ? kc : lc;
    return (Pd + Qd) * 0.5;
}
uint naga_div(uint lhs, uint rhs) {
    return lhs / metal::select(rhs, 1u, rhs == 0u);
}


struct eInput {
};
kernel void e(
  metal::uint3 he [[threadgroup_position_in_grid]]
, metal::uint3 ie [[thread_position_in_threadgroup]]
, constant ve& F [[buffer(0)]]
, device type_1 const& N [[buffer(1)]]
, device type_3 const& i [[buffer(2)]]
, device type_5& ua [[buffer(3)]]
, device type_5& r [[buffer(4)]]
, device type_6 const& va [[buffer(5)]]
, device type_3 const& ib [[buffer(6)]]
, device type_3 const& l [[buffer(7)]]
, threadgroup type_8& j
, constant _mslBufferSizes& _buffer_sizes [[buffer(8)]]
) {
    if (metal::all(ie == metal::uint3(0u))) {
        j = {};
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint Ha = 0u;
    uint ja = 0u;
    float J = {};
    uint Eb = 0u;
    uint Fb = {};
    uint ka = 0u;
    float Sc = {};
    uint la = {};
    uint Gb = 32u;
    uint Ia = {};
    uint ma = 0u;
    uint na = {};
    float Ka = {};
    uint _e2 = X(he);
    uint _e5 = F.c;
    if (_e2 >= _e5) {
        return;
    }
    xe y = va[metal::min(unsigned(_e2), (_buffer_sizes.size5 - 0 - 16) / 16)];
    we Kc = N[metal::min(unsigned(y.stream), (_buffer_sizes.size1 - 0 - 24) / 24)];
    uint k = ie.x;
    uint2 loop_bound_1 = uint2(4294967295u);
    bool loop_init_1 = true;
    while(true) {
        if (metal::all(loop_bound_1 == uint2(0u))) { break; }
        loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
        if (!loop_init_1) {
            uint _e37 = Ha;
            Ha = _e37 + 1u;
        }
        loop_init_1 = false;
        uint _e17 = Ha;
        if (_e17 < y.count) {
        } else {
            break;
        }
        {
            uint _e21 = Ha;
            uint _e29 = Ha;
            float _e35 = ib[metal::min(unsigned(((y.start + _e29) * 32u) + k), (_buffer_sizes.size6 - 0 - 4) / 4)];
            j.inner[metal::min(unsigned((_e21 * 32u) + k), 1023u)].x = _e35;
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint2 loop_bound_2 = uint2(4294967295u);
    bool loop_init_2 = true;
    while(true) {
        if (metal::all(loop_bound_2 == uint2(0u))) { break; }
        loop_bound_2 -= uint2(loop_bound_2.y == 0u, 1u);
        if (!loop_init_2) {
            uint _e63 = ja;
            ja = _e63 + 1u;
        }
        loop_init_2 = false;
        uint _e41 = ja;
        if (_e41 < y.count) {
        } else {
            break;
        }
        {
            uint _e44 = ja;
            metal::float2 _e45 = Vb(_e44, k, i, j, _buffer_sizes);
            uint _e47 = ja;
            j.inner[metal::min(unsigned((_e47 * 32u) + k), 1023u)].y = _e45.x;
            uint _e55 = ja;
            j.inner[metal::min(unsigned((_e55 * 32u) + k), 1023u)].z = _e45.y;
        }
    }
    uint Mc = ((_e2 * 32u) + k) * 4u;
    float _e74 = l[metal::min(unsigned(Mc + 1u), (_buffer_sizes.size7 - 0 - 4) / 4)];
    J = _e74;
    uint2 loop_bound_3 = uint2(4294967295u);
    bool loop_init_3 = true;
    while(true) {
        if (metal::all(loop_bound_3 == uint2(0u))) { break; }
        loop_bound_3 -= uint2(loop_bound_3.y == 0u, 1u);
        if (!loop_init_3) {
            uint _e98 = Eb;
            Eb = _e98 + 1u;
        }
        loop_init_3 = false;
        uint _e78 = Eb;
        if (_e78 < y.count) {
        } else {
            break;
        }
        {
            uint _e81 = Eb;
            uint Nc = (_e81 * 32u) + k;
            metal::float4 Oc = j.inner[metal::min(unsigned(Nc), 1023u)];
            float _e89 = J;
            J = (Oc.y * _e89) + Oc.z;
            float _e96 = J;
            j.inner[metal::min(unsigned(Nc), 1023u)].w = _e96;
        }
    }
    float _e104 = l[metal::min(unsigned(Mc + 3u), (_buffer_sizes.size7 - 0 - 4) / 4)];
    J = _e104;
    Fb = y.count;
    uint2 loop_bound_4 = uint2(4294967295u);
    bool loop_init_4 = true;
    while(true) {
        if (metal::all(loop_bound_4 == uint2(0u))) { break; }
        loop_bound_4 -= uint2(loop_bound_4.y == 0u, 1u);
        if (!loop_init_4) {
            uint _e129 = Fb;
            Fb = _e129 - 1u;
        }
        loop_init_4 = false;
        uint _e107 = Fb;
        if (_e107 > 0u) {
        } else {
            break;
        }
        {
            uint _e110 = Fb;
            uint Pc = ((_e110 - 1u) * 32u) + k;
            metal::float4 Qc = j.inner[metal::min(unsigned(Pc), 1023u)];
            float _e120 = J;
            J = (Qc.y * _e120) + Qc.z;
            float _e127 = J;
            j.inner[metal::min(unsigned(Pc), 1023u)].z = _e127;
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint2 loop_bound_5 = uint2(4294967295u);
    bool loop_init_5 = true;
    while(true) {
        if (metal::all(loop_bound_5 == uint2(0u))) { break; }
        loop_bound_5 -= uint2(loop_bound_5.y == 0u, 1u);
        if (!loop_init_5) {
            uint _e209 = ka;
            ka = _e209 + 1u;
        }
        loop_init_5 = false;
        uint _e133 = ka;
        if (_e133 < y.count) {
        } else {
            break;
        }
        {
            uint _e137 = ka;
            uint Rc = y.start + _e137;
            float _e144 = ib[metal::min(unsigned((Rc * 32u) + k), (_buffer_sizes.size6 - 0 - 4) / 4)];
            float _e149 = i[metal::min(unsigned(41289u + k), (_buffer_sizes.size2 - 0 - 4) / 4)];
            Sc = _e144 + _e149;
            uint Tc = 39241u + (k * 64u);
            la = 0u;
            uint2 loop_bound_6 = uint2(4294967295u);
            bool loop_init_6 = true;
            while(true) {
                if (metal::all(loop_bound_6 == uint2(0u))) { break; }
                loop_bound_6 -= uint2(loop_bound_6.y == 0u, 1u);
                if (!loop_init_6) {
                    uint _e189 = la;
                    la = _e189 + 1u;
                }
                loop_init_6 = false;
                uint _e158 = la;
                if (_e158 < 32u) {
                } else {
                    break;
                }
                {
                    uint _e162 = ka;
                    uint _e165 = la;
                    metal::float4 Uc = j.inner[metal::min(unsigned((_e162 * 32u) + _e165), 1023u)];
                    float _e169 = Sc;
                    uint _e172 = la;
                    float _e175 = i[metal::min(unsigned(Tc + _e172), (_buffer_sizes.size2 - 0 - 4) / 4)];
                    uint _e181 = la;
                    float _e184 = i[metal::min(unsigned((Tc + 32u) + _e181), (_buffer_sizes.size2 - 0 - 4) / 4)];
                    Sc = _e169 + ((Uc.w * _e175) + (Uc.z * _e184));
                }
            }
            float _e191 = Sc;
            float Vc = static_cast<float>(static_cast<half>(metal::tanh(_e191)));
            uint _e196 = ka;
            j.inner[metal::min(unsigned((_e196 * 32u) + k), 1023u)].y = Vc;
            ua[metal::min(unsigned((Rc * 32u) + k), (_buffer_sizes.size3 - 0 - 2) / 2)] = static_cast<half>(Vc);
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    Ia = y.count;
    uint2 loop_bound_7 = uint2(4294967295u);
    while(true) {
        if (metal::all(loop_bound_7 == uint2(0u))) { break; }
        loop_bound_7 -= uint2(loop_bound_7.y == 0u, 1u);
        uint _e217 = Gb;
        if (_e217 <= 1u) {
            break;
        }
        uint _e220 = Gb;
        uint Wc = naga_div(_e220, 2u);
        na = 0u;
        uint2 loop_bound_8 = uint2(4294967295u);
        bool loop_init_7 = true;
        while(true) {
            if (metal::all(loop_bound_8 == uint2(0u))) { break; }
            loop_bound_8 -= uint2(loop_bound_8.y == 0u, 1u);
            if (!loop_init_7) {
                uint _e321 = na;
                na = _e321 + 1u;
            }
            loop_init_7 = false;
            uint _e225 = na;
            if (_e225 < Wc) {
            } else {
                break;
            }
            {
                uint _e227 = na;
                uint Ja = _e227 * 2u;
                Ka = 0.0;
                uint _e232 = Ia;
                if (Ja < _e232) {
                    uint La = Ja * 32u;
                    uint _e236 = ma;
                    bool Ma = (_e236 & 1u) == 0u;
                    float _e245 = j.inner[metal::min(unsigned(La + k), 1023u)].x;
                    float _e250 = j.inner[metal::min(unsigned(La + k), 1023u)].y;
                    float Xc = Ma ? _e250 : _e245;
                    Ka = Xc;
                    uint _e254 = Ia;
                    if ((Ja + 1u) < _e254) {
                        uint Na = (Ja + 1u) * 32u;
                        uint _e261 = ma;
                        uint Oa = k ^ (1u << _e261);
                        float _e268 = j.inner[metal::min(unsigned(Na + k), 1023u)].x;
                        float _e273 = j.inner[metal::min(unsigned(Na + k), 1023u)].y;
                        float _e279 = j.inner[metal::min(unsigned(La + Oa), 1023u)].x;
                        float _e284 = j.inner[metal::min(unsigned(La + Oa), 1023u)].y;
                        float _e290 = j.inner[metal::min(unsigned(Na + Oa), 1023u)].x;
                        float _e295 = j.inner[metal::min(unsigned(Na + Oa), 1023u)].y;
                        uint _e297 = ma;
                        float _e298 = hb(Xc, Ma ? _e273 : _e268, Ma ? _e284 : _e279, Ma ? _e295 : _e290, k, _e297, i, _buffer_sizes);
                        Ka = _e298;
                    }
                }
                uint _e299 = ma;
                if ((_e299 & 1u) == 0u) {
                    uint _e305 = na;
                    float _e311 = Ka;
                    j.inner[metal::min(unsigned((_e305 * 32u) + k), 1023u)].x = _e311;
                } else {
                    uint _e313 = na;
                    float _e319 = Ka;
                    j.inner[metal::min(unsigned((_e313 * 32u) + k), 1023u)].y = _e319;
                }
            }
        }
        metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        Gb = Wc;
        uint _e323 = Ia;
        Ia = naga_div(_e323 + 1u, 2u);
        uint _e328 = ma;
        ma = _e328 + 1u;
    }
    uint je = ((Kc.f + Kc.g) - 1u) + y.h;
    float _e346 = j.inner[metal::min(unsigned(k), 1023u)].x;
    r[metal::min(unsigned((je * 32u) + k), (_buffer_sizes.size4 - 0 - 2) / 2)] = static_cast<half>(_e346);
    return;
}

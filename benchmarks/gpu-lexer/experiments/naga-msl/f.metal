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
typedef half type_5[1];

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

float ta(
    float nc,
    float Rd,
    float Sd,
    float Td,
    float Ud,
    float Vd,
    uint Wd,
    uint Xd,
    bool Yd,
    device type_3 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    uint A = (metal::min(Xd, 11u) * 32u) + Wd;
    float _e18 = i[metal::min(unsigned(28872u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e23 = i[metal::min(unsigned(29256u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float Zd = Yd ? _e23 : _e18;
    float _e29 = i[metal::min(unsigned(35472u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e35 = i[metal::min(unsigned(26568u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e42 = i[metal::min(unsigned(26952u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e49 = i[metal::min(unsigned(27336u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e56 = i[metal::min(unsigned(27720u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e63 = i[metal::min(unsigned(28104u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float ae = metal::tanh(((((((nc * _e29) + (Rd * _e35)) + (Sd * _e42)) + (Td * _e49)) + (Ud * _e56)) + (Vd * _e63)) + Zd);
    float _e72 = i[metal::min(unsigned(28488u + A), (_buffer_sizes.size2 - 0 - 4) / 4)];
    float _e73 = fb(_e72);
    return (nc * _e73) + (ae * (1.0 - _e73));
}
uint naga_div(uint lhs, uint rhs) {
    return lhs / metal::select(rhs, 1u, rhs == 0u);
}


struct fInput {
};
kernel void f(
  metal::uint3 ke [[threadgroup_position_in_grid]]
, metal::uint3 Yc [[thread_position_in_threadgroup]]
, constant ve& F [[buffer(0)]]
, device type_1 const& N [[buffer(1)]]
, device type_3 const& i [[buffer(2)]]
, device type_5& r [[buffer(3)]]
, device type_3& l [[buffer(4)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(5)]]
) {
    uint Hb = {};
    uint v = {};
    uint Qa = {};
    uint Ib = 5u;
    uint Sa = {};
    float Kb = {};
    uint S = {};
    uint oa = {};
    uint _e2 = X(ke);
    uint o = Yc.x & 31u;
    uint Pa = Yc.x >> 5u;
    uint _e11 = F.a;
    if (_e2 >= _e11) {
        return;
    }
    we p = N[metal::min(unsigned(_e2), (_buffer_sizes.size1 - 0 - 24) / 24)];
    uint le = (p.f + p.g) - 1u;
    Hb = p.c + Pa;
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e35 = Hb;
            Hb = _e35 + 8u;
        }
        loop_init = false;
        uint _e24 = Hb;
        if (_e24 < p.g) {
        } else {
            break;
        }
        {
            uint _e28 = Hb;
            r[metal::min(unsigned(((le + _e28) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)] = 0.0h;
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_device);
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    v = p.g;
    Qa = p.c;
    uint2 loop_bound_1 = uint2(4294967295u);
    while(true) {
        if (metal::all(loop_bound_1 == uint2(0u))) { break; }
        loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
        uint _e44 = v;
        if (_e44 <= 1u) {
            break;
        }
        uint _e47 = v;
        uint Jb = naga_div(_e47, 2u);
        uint me = (p.f + Jb) - 1u;
        uint _e55 = v;
        uint Ra = (p.f + _e55) - 1u;
        uint _e60 = Ib;
        uint ad = o ^ (1u << metal::min(_e60, 4u));
        Sa = Pa;
        uint2 loop_bound_2 = uint2(4294967295u);
        bool loop_init_1 = true;
        while(true) {
            if (metal::all(loop_bound_2 == uint2(0u))) { break; }
            loop_bound_2 -= uint2(loop_bound_2.y == 0u, 1u);
            if (!loop_init_1) {
                uint _e126 = Sa;
                Sa = _e126 + 8u;
            }
            loop_init_1 = false;
            uint _e66 = Sa;
            if (_e66 < Jb) {
            } else {
                break;
            }
            {
                uint _e68 = Sa;
                uint R = _e68 * 2u;
                Kb = 0.0;
                uint _e73 = Qa;
                if (R < _e73) {
                    half _e81 = r[metal::min(unsigned(((Ra + R) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)];
                    float bd = static_cast<float>(_e81);
                    Kb = bd;
                    uint _e85 = Qa;
                    if ((R + 1u) < _e85) {
                        half _e95 = r[metal::min(unsigned((((Ra + R) + 1u) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)];
                        half _e103 = r[metal::min(unsigned(((Ra + R) * 32u) + ad), (_buffer_sizes.size3 - 0 - 2) / 2)];
                        half _e113 = r[metal::min(unsigned((((Ra + R) + 1u) * 32u) + ad), (_buffer_sizes.size3 - 0 - 2) / 2)];
                        uint _e115 = Ib;
                        float _e116 = hb(bd, static_cast<float>(_e95), static_cast<float>(_e103), static_cast<float>(_e113), o, _e115, i, _buffer_sizes);
                        Kb = _e116;
                    }
                }
                uint _e118 = Sa;
                float _e124 = Kb;
                r[metal::min(unsigned(((me + _e118) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)] = static_cast<half>(_e124);
            }
        }
        metal::threadgroup_barrier(metal::mem_flags::mem_device);
        metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        v = Jb;
        uint _e129 = Qa;
        Qa = naga_div(_e129 + 1u, 2u);
        uint _e134 = Ib;
        Ib = _e134 + 1u;
    }
    if (Pa == 0u) {
        half _e151 = r[metal::min(unsigned((p.f * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)];
        l[metal::min(unsigned((p.f * 32u) + o), (_buffer_sizes.size4 - 0 - 4) / 4)] = static_cast<float>(static_cast<half>(static_cast<float>(_e151)));
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_device);
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    v = 1u;
    S = 4u + (31u - metal::clz(p.g));
    uint2 loop_bound_3 = uint2(4294967295u);
    while(true) {
        if (metal::all(loop_bound_3 == uint2(0u))) { break; }
        loop_bound_3 -= uint2(loop_bound_3.y == 0u, 1u);
        uint _e163 = v;
        if (_e163 >= p.g) {
            break;
        }
        uint _e167 = v;
        uint cd = (p.f + _e167) - 1u;
        uint _e172 = v;
        uint K = (p.f + (_e172 * 2u)) - 1u;
        uint _e179 = v;
        uint dd = naga_div(p.g, _e179 * 2u);
        uint ed = naga_div((p.c + dd) - 1u, dd);
        oa = Pa;
        uint2 loop_bound_4 = uint2(4294967295u);
        bool loop_init_2 = true;
        while(true) {
            if (metal::all(loop_bound_4 == uint2(0u))) { break; }
            loop_bound_4 -= uint2(loop_bound_4.y == 0u, 1u);
            if (!loop_init_2) {
                uint _e289 = oa;
                oa = _e289 + 8u;
            }
            loop_init_2 = false;
            uint _e189 = oa;
            uint _e190 = v;
            if (_e189 < _e190) {
            } else {
                break;
            }
            {
                uint _e192 = oa;
                uint B = _e192 * 2u;
                if (B >= ed) {
                    continue;
                }
                uint _e197 = oa;
                float Lb = l[metal::min(unsigned(((cd + _e197) * 32u) + o), (_buffer_sizes.size4 - 0 - 4) / 4)];
                half _e210 = r[metal::min(unsigned(((K + B) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)];
                float fd = static_cast<float>(_e210);
                if ((B + 1u) < ed) {
                    half _e223 = r[metal::min(unsigned((((K + B) + 1u) * 32u) + o), (_buffer_sizes.size3 - 0 - 2) / 2)];
                    float gd = static_cast<float>(_e223);
                    uint _e226 = S;
                    uint Mb = o ^ (1u << metal::min(_e226, 4u));
                    uint _e232 = oa;
                    float hd = l[metal::min(unsigned(((cd + _e232) * 32u) + Mb), (_buffer_sizes.size4 - 0 - 4) / 4)];
                    half _e245 = r[metal::min(unsigned(((K + B) * 32u) + Mb), (_buffer_sizes.size3 - 0 - 2) / 2)];
                    float id = static_cast<float>(_e245);
                    half _e255 = r[metal::min(unsigned((((K + B) + 1u) * 32u) + Mb), (_buffer_sizes.size3 - 0 - 2) / 2)];
                    float jd = static_cast<float>(_e255);
                    uint _e263 = S;
                    float _e265 = ta(Lb, fd, gd, hd, id, jd, o, _e263, false, i, _buffer_sizes);
                    l[metal::min(unsigned(((K + B) * 32u) + o), (_buffer_sizes.size4 - 0 - 4) / 4)] = static_cast<float>(static_cast<half>(_e265));
                    uint _e276 = S;
                    float _e278 = ta(Lb, gd, fd, hd, jd, id, o, _e276, true, i, _buffer_sizes);
                    l[metal::min(unsigned((((K + B) + 1u) * 32u) + o), (_buffer_sizes.size4 - 0 - 4) / 4)] = static_cast<float>(static_cast<half>(_e278));
                } else {
                    l[metal::min(unsigned(((K + B) * 32u) + o), (_buffer_sizes.size4 - 0 - 4) / 4)] = static_cast<float>(static_cast<half>(Lb));
                }
            }
        }
        metal::threadgroup_barrier(metal::mem_flags::mem_device);
        metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        uint _e292 = v;
        v = _e292 * 2u;
        uint _e296 = S;
        uint _e299 = S;
        S = (_e299 > 0u) ? (_e296 - 1u) : 0u;
    }
    return;
}

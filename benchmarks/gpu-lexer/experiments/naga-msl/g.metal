#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size0;
    uint size1;
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
typedef metal::atomic_uint type_3[1];
typedef we type_4[1];
typedef float type_6[1];
typedef half type_8[1];
typedef xe type_9[1];
struct type_10 {
    float inner[2016];
};
struct type_11 {
    float inner[128];
};
struct type_12 {
    float inner[576];
};
struct type_13 {
    float inner[72];
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

float hb(
    float kc,
    float lc,
    float Md,
    float Nd,
    uint mc,
    uint Od,
    device type_6 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    uint aa = (metal::min(Od, 11u) * 32u) + mc;
    float _e15 = i[metal::min(unsigned(24000u + aa), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e21 = i[metal::min(unsigned(24384u + aa), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e28 = i[metal::min(unsigned(24768u + aa), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e35 = i[metal::min(unsigned(25152u + aa), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e42 = i[metal::min(unsigned(25536u + aa), (_buffer_sizes.size4 - 0 - 4) / 4)];
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
    device type_6 const& i,
    constant _mslBufferSizes& _buffer_sizes
) {
    uint A = (metal::min(Xd, 11u) * 32u) + Wd;
    float _e18 = i[metal::min(unsigned(28872u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e23 = i[metal::min(unsigned(29256u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float Zd = Yd ? _e23 : _e18;
    float _e29 = i[metal::min(unsigned(35472u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e35 = i[metal::min(unsigned(26568u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e42 = i[metal::min(unsigned(26952u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e49 = i[metal::min(unsigned(27336u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e56 = i[metal::min(unsigned(27720u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e63 = i[metal::min(unsigned(28104u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float ae = metal::tanh(((((((nc * _e29) + (Rd * _e35)) + (Sd * _e42)) + (Td * _e49)) + (Ud * _e56)) + (Vd * _e63)) + Zd);
    float _e72 = i[metal::min(unsigned(28488u + A), (_buffer_sizes.size4 - 0 - 4) / 4)];
    float _e73 = fb(_e72);
    return (nc * _e73) + (ae * (1.0 - _e73));
}
uint naga_div(uint lhs, uint rhs) {
    return lhs / metal::select(rhs, 1u, rhs == 0u);
}

uint naga_mod(uint lhs, uint rhs) {
    return lhs % metal::select(rhs, 1u, rhs == 0u);
}


struct gInput {
};
kernel void g(
  metal::uint3 ne [[threadgroup_position_in_grid]]
, metal::uint3 Ta [[thread_position_in_threadgroup]]
, device type_1 const& M [[buffer(0)]]
, device type_3& Fd [[buffer(1)]]
, constant ve& F [[buffer(2)]]
, device type_4 const& N [[buffer(3)]]
, device type_6 const& i [[buffer(4)]]
, device type_8 const& ua [[buffer(5)]]
, device type_9 const& va [[buffer(6)]]
, device type_6 const& l [[buffer(7)]]
, threadgroup type_10& m
, threadgroup type_11& Wb
, threadgroup type_12& Xb
, threadgroup type_13& jb
, constant _mslBufferSizes& _buffer_sizes [[buffer(8)]]
) {
    if (metal::all(Ta == metal::uint3(0u))) {
        m = {};
        Wb = {};
        Xb = {};
        jb = {};
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    uint Va = 0u;
    uint w = 32u;
    uint Wa = {};
    uint z = 0u;
    uint Ya = {};
    float Ob = {};
    uint pa = {};
    uint Rb = 0u;
    bool local = {};
    bool local_1 = {};
    uint ra = {};
    bool local_2 = {};
    float xd = {};
    uint U = {};
    uint V = {};
    bool local_3 = {};
    float Tb = {};
    uint W = {};
    uint cb = {};
    uint sa = {};
    bool local_4 = {};
    float Ad = {};
    uint db = {};
    bool local_5 = {};
    uint Bd = {};
    float Cd = {};
    uint eb = {};
    uint _e2 = X(ne);
    uint _e5 = F.c;
    if (_e2 >= _e5) {
        return;
    }
    xe D = va[metal::min(unsigned(_e2), (_buffer_sizes.size6 - 0 - 16) / 16)];
    we ld = N[metal::min(unsigned(D.stream), (_buffer_sizes.size3 - 0 - 24) / 24)];
    bool Ua = Ta.x < 32u;
    uint n = Ta.x;
    if (Ua) {
        uint2 loop_bound = uint2(4294967295u);
        bool loop_init = true;
        while(true) {
            if (metal::all(loop_bound == uint2(0u))) { break; }
            loop_bound -= uint2(loop_bound.y == 0u, 1u);
            if (!loop_init) {
                uint _e42 = Va;
                Va = _e42 + 1u;
            }
            loop_init = false;
            uint _e20 = Va;
            if (_e20 < D.count) {
            } else {
                break;
            }
            {
                uint _e25 = Va;
                uint _e33 = Va;
                half _e39 = ua[metal::min(unsigned(((D.start + _e33) * 32u) + n), (_buffer_sizes.size5 - 0 - 2) / 2)];
                m.inner[metal::min(unsigned(((31u + _e25) * 32u) + n), 2015u)] = static_cast<float>(_e39);
            }
        }
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    Wa = D.count;
    uint2 loop_bound_1 = uint2(4294967295u);
    while(true) {
        if (metal::all(loop_bound_1 == uint2(0u))) { break; }
        loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
        uint _e50 = w;
        if (_e50 <= 1u) {
            break;
        }
        uint _e53 = w;
        uint Nb = naga_div(_e53, 2u);
        uint oe = Nb - 1u;
        uint _e58 = w;
        uint Xa = _e58 - 1u;
        if (Ua) {
            Ya = 0u;
            uint2 loop_bound_2 = uint2(4294967295u);
            bool loop_init_1 = true;
            while(true) {
                if (metal::all(loop_bound_2 == uint2(0u))) { break; }
                loop_bound_2 -= uint2(loop_bound_2.y == 0u, 1u);
                if (!loop_init_1) {
                    uint _e123 = Ya;
                    Ya = _e123 + 1u;
                }
                loop_init_1 = false;
                uint _e63 = Ya;
                if (_e63 < Nb) {
                } else {
                    break;
                }
                {
                    uint _e65 = Ya;
                    uint T = _e65 * 2u;
                    Ob = 0.0;
                    uint _e70 = Wa;
                    if (T < _e70) {
                        float md = m.inner[metal::min(unsigned(((Xa + T) * 32u) + n), 2015u)];
                        Ob = md;
                        uint _e81 = Wa;
                        if ((T + 1u) < _e81) {
                            uint _e84 = z;
                            uint nd = n ^ (1u << _e84);
                            float _e95 = m.inner[metal::min(unsigned((((Xa + T) + 1u) * 32u) + n), 2015u)];
                            float _e102 = m.inner[metal::min(unsigned(((Xa + T) * 32u) + nd), 2015u)];
                            float _e111 = m.inner[metal::min(unsigned((((Xa + T) + 1u) * 32u) + nd), 2015u)];
                            uint _e112 = z;
                            float _e113 = hb(md, _e95, _e102, _e111, n, _e112, i, _buffer_sizes);
                            Ob = _e113;
                        }
                    }
                    uint _e115 = Ya;
                    float _e121 = Ob;
                    m.inner[metal::min(unsigned(((oe + _e115) * 32u) + n), 2015u)] = _e121;
                }
            }
        }
        metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        w = Nb;
        uint _e125 = Wa;
        Wa = naga_div(_e125 + 1u, 2u);
        uint _e130 = z;
        z = _e130 + 1u;
    }
    if (Ua) {
        uint pe = ((ld.f + ld.g) - 1u) + D.h;
        float _e147 = l[metal::min(unsigned((pe * 32u) + n), (_buffer_sizes.size7 - 0 - 4) / 4)];
        m.inner[metal::min(unsigned(n), 2015u)] = _e147;
    }
    metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
    w = 1u;
    z = 4u;
    uint2 loop_bound_3 = uint2(4294967295u);
    while(true) {
        if (metal::all(loop_bound_3 == uint2(0u))) { break; }
        loop_bound_3 -= uint2(loop_bound_3.y == 0u, 1u);
        uint _e150 = w;
        if (_e150 >= 32u) {
            break;
        }
        uint _e153 = w;
        uint od = _e153 - 1u;
        uint _e156 = w;
        uint L = (_e156 * 2u) - 1u;
        uint _e162 = w;
        uint pd = naga_div(32u, _e162 * 2u);
        uint qd = naga_div((D.count + pd) - 1u, pd);
        if (Ua) {
            pa = 0u;
            uint2 loop_bound_4 = uint2(4294967295u);
            bool loop_init_2 = true;
            while(true) {
                if (metal::all(loop_bound_4 == uint2(0u))) { break; }
                loop_bound_4 -= uint2(loop_bound_4.y == 0u, 1u);
                if (!loop_init_2) {
                    uint _e262 = pa;
                    pa = _e262 + 1u;
                }
                loop_init_2 = false;
                uint _e173 = pa;
                uint _e174 = w;
                if (_e173 < _e174) {
                } else {
                    break;
                }
                {
                    uint _e176 = pa;
                    uint C = _e176 * 2u;
                    if (C >= qd) {
                        continue;
                    }
                    uint _e181 = pa;
                    float Pb = m.inner[metal::min(unsigned(((od + _e181) * 32u) + n), 2015u)];
                    float rd = m.inner[metal::min(unsigned(((L + C) * 32u) + n), 2015u)];
                    if ((C + 1u) < qd) {
                        float sd = m.inner[metal::min(unsigned((((L + C) + 1u) * 32u) + n), 2015u)];
                        uint _e208 = z;
                        uint Qb = n ^ (1u << _e208);
                        uint _e212 = pa;
                        float td = m.inner[metal::min(unsigned(((od + _e212) * 32u) + Qb), 2015u)];
                        float ud = m.inner[metal::min(unsigned(((L + C) * 32u) + Qb), 2015u)];
                        float vd = m.inner[metal::min(unsigned((((L + C) + 1u) * 32u) + Qb), 2015u)];
                        uint _e241 = z;
                        float _e243 = ta(Pb, rd, sd, td, ud, vd, n, _e241, false, i, _buffer_sizes);
                        m.inner[metal::min(unsigned(((L + C) * 32u) + n), 2015u)] = _e243;
                        uint _e252 = z;
                        float _e254 = ta(Pb, sd, rd, td, vd, ud, n, _e252, true, i, _buffer_sizes);
                        m.inner[metal::min(unsigned((((L + C) + 1u) * 32u) + n), 2015u)] = _e254;
                    } else {
                        m.inner[metal::min(unsigned(((L + C) * 32u) + n), 2015u)] = Pb;
                    }
                }
            }
        }
        metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        uint _e264 = w;
        w = _e264 * 2u;
        uint _e268 = z;
        uint _e271 = z;
        z = (_e271 > 0u) ? (_e268 - 1u) : 0u;
    }
    uint E = naga_div(Ta.x, 8u);
    uint Za = naga_mod(Ta.x, 8u);
    uint2 loop_bound_5 = uint2(4294967295u);
    bool loop_init_3 = true;
    while(true) {
        if (metal::all(loop_bound_5 == uint2(0u))) { break; }
        loop_bound_5 -= uint2(loop_bound_5.y == 0u, 1u);
        if (!loop_init_3) {
            uint _e592 = Rb;
            Rb = _e592 + 1u;
        }
        loop_init_3 = false;
        uint _e283 = Rb;
        if (_e283 < 4u) {
        } else {
            break;
        }
        {
            uint _e286 = Rb;
            uint ab = (_e286 * 8u) + E;
            uint qa = D.start + ab;
            bool Sb = ab < D.count;
            uint _e299 = M[metal::min(unsigned(qa * 2u), (_buffer_sizes.size0 - 0 - 4) / 4)];
            uint wd = Sb ? (_e299 & 3u) : 1u;
            if (Sb) {
                local = wd != 1u;
            } else {
                local = false;
            }
            bool _e308 = local;
            if (_e308) {
                local_1 = wd != 2u;
            } else {
                local_1 = false;
            }
            bool bb = local_1;
            ra = Za;
            uint2 loop_bound_6 = uint2(4294967295u);
            bool loop_init_4 = true;
            while(true) {
                if (metal::all(loop_bound_6 == uint2(0u))) { break; }
                loop_bound_6 -= uint2(loop_bound_6.y == 0u, 1u);
                if (!loop_init_4) {
                    uint _e387 = ra;
                    ra = _e387 + 8u;
                }
                loop_init_4 = false;
                if (bb) {
                    uint _e318 = ra;
                    local_2 = _e318 < 16u;
                } else {
                    local_2 = false;
                }
                bool _e322 = local_2;
                if (_e322) {
                } else {
                    break;
                }
                {
                    uint _e325 = ra;
                    float _e328 = i[metal::min(unsigned(36889u + _e325), (_buffer_sizes.size4 - 0 - 4) / 4)];
                    xd = _e328;
                    U = 0u;
                    uint2 loop_bound_7 = uint2(4294967295u);
                    bool loop_init_5 = true;
                    while(true) {
                        if (metal::all(loop_bound_7 == uint2(0u))) { break; }
                        loop_bound_7 -= uint2(loop_bound_7.y == 0u, 1u);
                        if (!loop_init_5) {
                            uint _e377 = U;
                            U = _e377 + 1u;
                        }
                        loop_init_5 = false;
                        uint _e332 = U;
                        if (_e332 < 32u) {
                        } else {
                            break;
                        }
                        {
                            uint _e338 = U;
                            half _e341 = ua[metal::min(unsigned((qa * 32u) + _e338), (_buffer_sizes.size5 - 0 - 2) / 2)];
                            float qe = static_cast<float>(_e341);
                            uint _e348 = U;
                            float _e351 = m.inner[metal::min(unsigned(((31u + ab) * 32u) + _e348), 2015u)];
                            float re = static_cast<float>(static_cast<half>(_e351));
                            uint _e355 = ra;
                            uint yd = 35865u + (_e355 * 64u);
                            float _e359 = xd;
                            uint _e361 = U;
                            float _e364 = i[metal::min(unsigned(yd + _e361), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            uint _e369 = U;
                            float _e372 = i[metal::min(unsigned((yd + 32u) + _e369), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            xd = _e359 + ((_e364 * qe) + (_e372 * re));
                        }
                    }
                    uint _e382 = ra;
                    float _e385 = xd;
                    float _e386 = fb(_e385);
                    Wb.inner[metal::min(unsigned((E * 16u) + _e382), 127u)] = _e386;
                }
            }
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
            V = Za;
            uint2 loop_bound_8 = uint2(4294967295u);
            bool loop_init_6 = true;
            while(true) {
                if (metal::all(loop_bound_8 == uint2(0u))) { break; }
                loop_bound_8 -= uint2(loop_bound_8.y == 0u, 1u);
                if (!loop_init_6) {
                    uint _e492 = V;
                    V = _e492 + 8u;
                }
                loop_init_6 = false;
                if (bb) {
                    uint _e393 = V;
                    local_3 = _e393 < 72u;
                } else {
                    local_3 = false;
                }
                bool _e397 = local_3;
                if (_e397) {
                } else {
                    break;
                }
                {
                    uint _e400 = V;
                    float _e403 = i[metal::min(unsigned(35400u + _e400), (_buffer_sizes.size4 - 0 - 4) / 4)];
                    Tb = _e403;
                    W = 0u;
                    uint2 loop_bound_9 = uint2(4294967295u);
                    bool loop_init_7 = true;
                    while(true) {
                        if (metal::all(loop_bound_9 == uint2(0u))) { break; }
                        loop_bound_9 -= uint2(loop_bound_9.y == 0u, 1u);
                        if (!loop_init_7) {
                            uint _e452 = W;
                            W = _e452 + 1u;
                        }
                        loop_init_7 = false;
                        uint _e407 = W;
                        if (_e407 < 32u) {
                        } else {
                            break;
                        }
                        {
                            uint _e413 = W;
                            half _e416 = ua[metal::min(unsigned((qa * 32u) + _e413), (_buffer_sizes.size5 - 0 - 2) / 2)];
                            float se = static_cast<float>(_e416);
                            uint _e423 = W;
                            float _e426 = m.inner[metal::min(unsigned(((31u + ab) * 32u) + _e423), 2015u)];
                            float te = static_cast<float>(static_cast<half>(_e426));
                            uint _e430 = V;
                            uint zd = 29640u + (_e430 * 80u);
                            float _e434 = Tb;
                            uint _e436 = W;
                            float _e439 = i[metal::min(unsigned(zd + _e436), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            uint _e444 = W;
                            float _e447 = i[metal::min(unsigned((zd + 32u) + _e444), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            Tb = _e434 + ((_e439 * se) + (_e447 * te));
                        }
                    }
                    uint _e455 = V;
                    uint ue = (29640u + (_e455 * 80u)) + 64u;
                    cb = 0u;
                    uint2 loop_bound_10 = uint2(4294967295u);
                    bool loop_init_8 = true;
                    while(true) {
                        if (metal::all(loop_bound_10 == uint2(0u))) { break; }
                        loop_bound_10 -= uint2(loop_bound_10.y == 0u, 1u);
                        if (!loop_init_8) {
                            uint _e482 = cb;
                            cb = _e482 + 1u;
                        }
                        loop_init_8 = false;
                        uint _e463 = cb;
                        if (_e463 < 16u) {
                        } else {
                            break;
                        }
                        {
                            float _e466 = Tb;
                            uint _e468 = cb;
                            float _e471 = i[metal::min(unsigned(ue + _e468), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            uint _e475 = cb;
                            float _e478 = Wb.inner[metal::min(unsigned((E * 16u) + _e475), 127u)];
                            Tb = _e466 + (_e471 * _e478);
                        }
                    }
                    uint _e487 = V;
                    float _e490 = Tb;
                    Xb.inner[metal::min(unsigned((E * 72u) + _e487), 575u)] = metal::tanh(_e490);
                }
            }
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
            sa = Za;
            uint2 loop_bound_11 = uint2(4294967295u);
            bool loop_init_9 = true;
            while(true) {
                if (metal::all(loop_bound_11 == uint2(0u))) { break; }
                loop_bound_11 -= uint2(loop_bound_11.y == 0u, 1u);
                if (!loop_init_9) {
                    uint _e545 = sa;
                    sa = _e545 + 8u;
                }
                loop_init_9 = false;
                if (bb) {
                    uint _e498 = sa;
                    local_4 = _e498 < 9u;
                } else {
                    local_4 = false;
                }
                bool _e502 = local_4;
                if (_e502) {
                } else {
                    break;
                }
                {
                    uint _e505 = sa;
                    float _e508 = i[metal::min(unsigned(35856u + _e505), (_buffer_sizes.size4 - 0 - 4) / 4)];
                    Ad = _e508;
                    db = 0u;
                    uint2 loop_bound_12 = uint2(4294967295u);
                    bool loop_init_10 = true;
                    while(true) {
                        if (metal::all(loop_bound_12 == uint2(0u))) { break; }
                        loop_bound_12 -= uint2(loop_bound_12.y == 0u, 1u);
                        if (!loop_init_10) {
                            uint _e536 = db;
                            db = _e536 + 1u;
                        }
                        loop_init_10 = false;
                        uint _e512 = db;
                        if (_e512 < 72u) {
                        } else {
                            break;
                        }
                        {
                            float _e515 = Ad;
                            uint _e518 = sa;
                            uint _e522 = db;
                            float _e525 = i[metal::min(unsigned((25920u + (_e518 * 72u)) + _e522), (_buffer_sizes.size4 - 0 - 4) / 4)];
                            uint _e529 = db;
                            float _e532 = Xb.inner[metal::min(unsigned((E * 72u) + _e529), 575u)];
                            Ad = _e515 + (_e525 * _e532);
                        }
                    }
                    uint _e541 = sa;
                    float _e544 = Ad;
                    jb.inner[metal::min(unsigned((E * 9u) + _e541), 71u)] = _e544;
                }
            }
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
            if (Za == 0u) {
                local_5 = Sb;
            } else {
                local_5 = false;
            }
            bool _e553 = local_5;
            if (_e553) {
                Bd = 0u;
                if (bb) {
                    float _e560 = jb.inner[metal::min(unsigned(E * 9u), 71u)];
                    Cd = _e560;
                    eb = 1u;
                    uint2 loop_bound_13 = uint2(4294967295u);
                    bool loop_init_11 = true;
                    while(true) {
                        if (metal::all(loop_bound_13 == uint2(0u))) { break; }
                        loop_bound_13 -= uint2(loop_bound_13.y == 0u, 1u);
                        if (!loop_init_11) {
                            uint _e578 = eb;
                            eb = _e578 + 1u;
                        }
                        loop_init_11 = false;
                        uint _e564 = eb;
                        if (_e564 < 9u) {
                        } else {
                            break;
                        }
                        {
                            uint _e570 = eb;
                            float Dd = jb.inner[metal::min(unsigned((E * 9u) + _e570), 71u)];
                            float _e574 = Cd;
                            if (Dd > _e574) {
                                Cd = Dd;
                                uint _e576 = eb;
                                Bd = _e576;
                            }
                        }
                    }
                }
                uint _e584 = Bd;
                uint _e590 = metal::atomic_fetch_or_explicit(&Fd[metal::min(unsigned(naga_div(qa, 4u)), (_buffer_sizes.size1 - 0 - 4) / 4)], _e584 << ((qa & 3u) * 8u), metal::memory_order_relaxed);
            }
            metal::threadgroup_barrier(metal::mem_flags::mem_threadgroup);
        }
    }
    return;
}

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
typedef metal::uint2 type_3[1];

uint Ub(
    metal::uint3 Zb
) {
    return Zb.x + (Zb.y * 4194240u);
}

struct bInput {
};
kernel void b(
  metal::uint3 ce [[thread_position_in_grid]]
, constant ve& F [[buffer(0)]]
, device type_1 const& N [[buffer(1)]]
, device type_3& G [[buffer(2)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(3)]]
) {
    uint tc = 4294967295u;
    uint pb = 0u;
    uint wc = 4294967295u;
    uint qb = {};
    uint _e1 = Ub(ce);
    uint _e4 = F.a;
    if (_e1 >= _e4) {
        return;
    }
    we Ca = N[metal::min(unsigned(_e1), (_buffer_sizes.size1 - 0 - 24) / 24)];
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e31 = pb;
            pb = _e31 + 1u;
        }
        loop_init = false;
        uint _e13 = pb;
        if (_e13 < Ca.c) {
        } else {
            break;
        }
        {
            uint _e17 = pb;
            uint uc = Ca.e + _e17;
            metal::uint2 vc = G[metal::min(unsigned(uc), (_buffer_sizes.size2 - 0 - 8) / 8)];
            uint _e25 = tc;
            G[metal::min(unsigned(uc), (_buffer_sizes.size2 - 0 - 8) / 8)].y = _e25;
            if (vc.y != 4294967295u) {
                tc = vc.y;
            }
        }
    }
    qb = Ca.c;
    uint2 loop_bound_1 = uint2(4294967295u);
    bool loop_init_1 = true;
    while(true) {
        if (metal::all(loop_bound_1 == uint2(0u))) { break; }
        loop_bound_1 -= uint2(loop_bound_1.y == 0u, 1u);
        if (!loop_init_1) {
            uint _e56 = qb;
            qb = _e56 - 1u;
        }
        loop_init_1 = false;
        uint _e37 = qb;
        if (_e37 > 0u) {
        } else {
            break;
        }
        {
            uint _e41 = qb;
            uint xc = (Ca.e + _e41) - 1u;
            uint yc = G[metal::min(unsigned(xc), (_buffer_sizes.size2 - 0 - 8) / 8)].x;
            uint _e52 = wc;
            G[metal::min(unsigned(xc), (_buffer_sizes.size2 - 0 - 8) / 8)].x = _e52;
            if (yc != 4294967295u) {
                wc = yc;
            }
        }
    }
    return;
}

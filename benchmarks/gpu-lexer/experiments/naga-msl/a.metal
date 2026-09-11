#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size0;
    uint size2;
    uint size3;
};

struct ve {
    uint a;
    uint b;
    uint c;
    uint d;
};
struct xe {
    uint start;
    uint count;
    uint stream;
    uint h;
};
typedef uint type_1[1];
typedef xe type_2[1];
typedef metal::uint2 type_4[1];

uint Ub(
    metal::uint3 Zb
) {
    return Zb.x + (Zb.y * 4194240u);
}

struct aInput {
};
kernel void a(
  metal::uint3 be [[thread_position_in_grid]]
, device type_1 const& M [[buffer(0)]]
, constant ve& F [[buffer(1)]]
, device type_2 const& va [[buffer(2)]]
, device type_4& G [[buffer(3)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(4)]]
) {
    uint mb = 4294967295u;
    uint qc = 4294967295u;
    uint nb = 0u;
    bool local = {};
    uint _e1 = Ub(be);
    uint _e4 = F.c;
    if (_e1 >= _e4) {
        return;
    }
    xe pc = va[metal::min(unsigned(_e1), (_buffer_sizes.size2 - 0 - 16) / 16)];
    uint2 loop_bound = uint2(4294967295u);
    bool loop_init = true;
    while(true) {
        if (metal::all(loop_bound == uint2(0u))) { break; }
        loop_bound -= uint2(loop_bound.y == 0u, 1u);
        if (!loop_init) {
            uint _e40 = nb;
            nb = _e40 + 1u;
        }
        loop_init = false;
        uint _e15 = nb;
        if (_e15 < pc.count) {
        } else {
            break;
        }
        {
            uint _e19 = nb;
            uint ob = pc.start + _e19;
            uint _e25 = M[metal::min(unsigned(ob * 2u), (_buffer_sizes.size0 - 0 - 4) / 4)];
            uint rc = _e25 & 3u;
            if (rc != 1u) {
                local = rc != 2u;
            } else {
                local = false;
            }
            bool _e35 = local;
            if (_e35) {
                uint _e36 = mb;
                if (_e36 == 4294967295u) {
                    mb = ob;
                }
                qc = ob;
            }
        }
    }
    uint _e44 = mb;
    uint _e45 = qc;
    G[metal::min(unsigned(_e1), (_buffer_sizes.size3 - 0 - 8) / 8)] = metal::uint2(_e44, _e45);
    return;
}

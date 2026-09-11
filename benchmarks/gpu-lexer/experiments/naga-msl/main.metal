#pragma METAL fp math_mode(relaxed)
// language: metal3.2
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size1;
    uint size2;
    uint size3;
};

struct MetadataEntry {
    uint src_offset;
    uint dst_offset;
    uint vertex_or_index_limit;
    uint instance_limit;
};
struct MetadataRange {
    uint start;
    uint count;
};
typedef MetadataEntry type_2[1];
typedef uint type_3[1];
constant bool supports_indirect_first_instance = false;
constant bool write_d3d12_special_constants = false;

bool is_bit_set(
    uint data,
    uint index
) {
    return ((data >> index) & 1u) == 1u;
}

struct main_Input {
};
kernel void main_(
  metal::uint3 global_invocation_id [[thread_position_in_grid]]
, constant MetadataRange& metadata_range [[buffer(0)]]
, device type_2 const& metadata [[buffer(1)]]
, device type_3 const& src [[buffer(2)]]
, device type_3& dst [[buffer(3)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(4)]]
) {
    bool failed = false;
    bool local = {};
    bool local_1 = {};
    uint _e4 = metadata_range.count;
    if (global_invocation_id.x >= _e4) {
        return;
    }
    uint _e9 = metadata_range.start;
    MetadataEntry metadata_1 = metadata[_e9 + global_invocation_id.x];
    bool _e18 = is_bit_set(metadata_1.src_offset, 31u);
    uint src_base_offset = (metadata_1.src_offset << 2u) >> 2u;
    uint dst_base_offset = (metadata_1.dst_offset << 2u) >> 2u;
    uint first_vertex_or_index = src[src_base_offset + 2u];
    uint vertex_or_index_count = src[src_base_offset + 0u];
    {
        bool _e41 = is_bit_set(metadata_1.dst_offset, 30u);
        bool sub_overflows = metadata_1.vertex_or_index_limit < first_vertex_or_index;
        bool _e44 = failed;
        if (sub_overflows) {
            local = !(_e41);
        } else {
            local = false;
        }
        bool _e49 = local;
        failed = _e44 | _e49;
        uint vertex_or_index_limit = metadata_1.vertex_or_index_limit - first_vertex_or_index;
        bool _e53 = failed;
        failed = _e53 | (vertex_or_index_limit < vertex_or_index_count);
    }
    uint first_instance = src[(src_base_offset + 3u) + static_cast<uint>(_e18)];
    uint instance_count = src[src_base_offset + 1u];
    {
        bool _e70 = is_bit_set(metadata_1.dst_offset, 31u);
        bool sub_overflows_1 = metadata_1.instance_limit < first_instance;
        bool _e73 = failed;
        if (sub_overflows_1) {
            local_1 = !(_e70);
        } else {
            local_1 = false;
        }
        bool _e78 = local_1;
        failed = _e73 | _e78;
        uint instance_limit = metadata_1.instance_limit - first_instance;
        bool _e82 = failed;
        failed = _e82 | (instance_limit < instance_count);
    }
    if (true) {
        bool _e88 = failed;
        failed = _e88 | (first_instance != 0u);
    }
    bool _e97 = failed;
    if (_e97) {
        if (write_d3d12_special_constants) {
            dst[dst_base_offset + 0u] = 0u;
            dst[dst_base_offset + 1u] = 0u;
            dst[dst_base_offset + 2u] = 0u;
        }
        dst[(dst_base_offset + 0u) + 0u] = 0u;
        dst[(dst_base_offset + 0u) + 1u] = 0u;
        dst[(dst_base_offset + 0u) + 2u] = 0u;
        dst[(dst_base_offset + 0u) + 3u] = 0u;
        if (_e18) {
            dst[(dst_base_offset + 0u) + 4u] = 0u;
            return;
        } else {
            return;
        }
    } else {
        if (write_d3d12_special_constants) {
            uint _e155 = src[(src_base_offset + 2u) + static_cast<uint>(_e18)];
            dst[dst_base_offset + 0u] = _e155;
            uint _e166 = src[(src_base_offset + 3u) + static_cast<uint>(_e18)];
            dst[dst_base_offset + 1u] = _e166;
            dst[dst_base_offset + 2u] = 0u;
        }
        uint _e181 = src[src_base_offset + 0u];
        dst[(dst_base_offset + 0u) + 0u] = _e181;
        uint _e191 = src[src_base_offset + 1u];
        dst[(dst_base_offset + 0u) + 1u] = _e191;
        uint _e201 = src[src_base_offset + 2u];
        dst[(dst_base_offset + 0u) + 2u] = _e201;
        uint _e211 = src[src_base_offset + 3u];
        dst[(dst_base_offset + 0u) + 3u] = _e211;
        if (_e18) {
            uint _e221 = src[src_base_offset + 4u];
            dst[(dst_base_offset + 0u) + 4u] = _e221;
            return;
        } else {
            return;
        }
    }
}

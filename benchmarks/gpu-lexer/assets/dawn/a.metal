/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

template<typename T, size_t N>
struct tint_array {
  const constant T& operator[](size_t i) const constant { return elements[i]; }
  device T& operator[](size_t i) device { return elements[i]; }
  const device T& operator[](size_t i) const device { return elements[i]; }
  thread T& operator[](size_t i) thread { return elements[i]; }
  const thread T& operator[](size_t i) const thread { return elements[i]; }
  threadgroup T& operator[](size_t i) threadgroup { return elements[i]; }
  const threadgroup T& operator[](size_t i) const threadgroup { return elements[i]; }
  T elements[N];
};

struct ve {
  /* 0x0000 */ uint a;
  /* 0x0004 */ uint b;
  /* 0x0008 */ uint c;
  /* 0x000c */ uint d;
};

struct xe {
  /* 0x0000 */ uint start;
  /* 0x0004 */ uint count;
  /* 0x0008 */ uint stream;
  /* 0x000c */ uint h;
};

struct tint_immediate_data_struct {
  /* 0x0000 */ tint_array<uint4, 1> tint_storage_buffer_sizes;
};

struct tint_module_vars_struct {
  const device tint_array<uint, 1>* M;
  const constant ve* F;
  const device tint_array<xe, 1>* va;
  device tint_array<uint2, 1>* G;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

struct tint_array_lengths_struct {
  uint tint_array_length_0_2;
  uint tint_array_length_0_0;
  uint tint_array_length_0_3;
};

uint Ub(uint3 Zb) {
  return (Zb.x + (Zb.y * 4194240u));
}

void a_inner(uint3 be, tint_module_vars_struct tint_module_vars) {
  tint_array_lengths_struct const v_1 = tint_array_lengths_struct{.tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 16u), .tint_array_length_0_0=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].x / 4u), .tint_array_length_0_3=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].w / 8u)};
  uint const lb = Ub(be);
  if ((lb >= (*tint_module_vars.F).c)) {
    return;
  }
  xe const pc = (*tint_module_vars.va)[min(lb, (v_1.tint_array_length_0_2 - 1u))];
  uint mb = 4294967295u;
  uint qc = 4294967295u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint nb = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((nb < pc.count)) {
      } else {
        break;
      }
      uint const ob = (pc.start + nb);
      uint const rc = ((*tint_module_vars.M)[min((ob * 2u), (v_1.tint_array_length_0_0 - 1u))] & 3u);
      bool v_2 = false;
      if ((rc != 1u)) {
        v_2 = (rc != 2u);
      } else {
        v_2 = false;
      }
      if (v_2) {
        if ((mb == 4294967295u)) {
          mb = ob;
        }
        qc = ob;
      }
      {
        uint const tint_low_inc = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc;
        uint const tint_carry = uint((tint_low_inc == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
        nb = (nb + 1u);
      }
    }
  }
  (*tint_module_vars.G)[min(lb, (v_1.tint_array_length_0_3 - 1u))] = uint2(mb, qc);
}

[[max_total_threads_per_threadgroup(64)]]
kernel void gpu_lexer_a(uint3 be [[thread_position_in_grid]], const device tint_array<uint, 1>* M [[buffer(0)]], const constant ve* F [[buffer(1)]], const device tint_array<xe, 1>* va [[buffer(2)]], device tint_array<uint2, 1>* G [[buffer(3)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.M=M, .F=F, .va=va, .G=G, .tint_immediate_data=tint_immediate_data};
  a_inner(be, tint_module_vars);
}

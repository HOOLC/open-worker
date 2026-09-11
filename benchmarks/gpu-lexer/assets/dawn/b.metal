/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

struct ve {
  /* 0x0000 */ uint a;
  /* 0x0004 */ uint b;
  /* 0x0008 */ uint c;
  /* 0x000c */ uint d;
};

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

struct we {
  /* 0x0000 */ uint start;
  /* 0x0004 */ uint count;
  /* 0x0008 */ uint e;
  /* 0x000c */ uint c;
  /* 0x0010 */ uint f;
  /* 0x0014 */ uint g;
};

struct tint_immediate_data_struct {
  /* 0x0000 */ tint_array<uint4, 1> tint_storage_buffer_sizes;
};

struct tint_module_vars_struct {
  const constant ve* F;
  const device tint_array<we, 1>* N;
  device tint_array<uint2, 1>* G;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

struct tint_array_lengths_struct {
  uint tint_array_length_0_1;
  uint tint_array_length_0_2;
};

uint Ub(uint3 Zb) {
  return (Zb.x + (Zb.y * 4194240u));
}

void b_inner(uint3 ce, tint_module_vars_struct tint_module_vars) {
  tint_array_lengths_struct const v_1 = tint_array_lengths_struct{.tint_array_length_0_1=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].y / 24u), .tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 8u)};
  uint const sc = Ub(ce);
  if ((sc >= (*tint_module_vars.F).a)) {
    return;
  }
  we const Ca = (*tint_module_vars.N)[min(sc, (v_1.tint_array_length_0_1 - 1u))];
  uint tc = 4294967295u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint pb = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((pb < Ca.c)) {
      } else {
        break;
      }
      uint const uc = (Ca.e + pb);
      uint2 const vc = (*tint_module_vars.G)[min(uc, (v_1.tint_array_length_0_2 - 1u))];
      (*tint_module_vars.G)[min(uc, (v_1.tint_array_length_0_2 - 1u))].y = tc;
      if ((vc.y != 4294967295u)) {
        tc = vc.y;
      }
      {
        uint const tint_low_inc = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc;
        uint const tint_carry = uint((tint_low_inc == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
        pb = (pb + 1u);
      }
    }
  }
  uint wc = 4294967295u;
  {
    uint qb = Ca.c;
    while(true) {
      if ((qb > 0u)) {
      } else {
        break;
      }
      uint const xc = ((Ca.e + qb) - 1u);
      uint const yc = (*tint_module_vars.G)[min(xc, (v_1.tint_array_length_0_2 - 1u))].x;
      (*tint_module_vars.G)[min(xc, (v_1.tint_array_length_0_2 - 1u))].x = wc;
      if ((yc != 4294967295u)) {
        wc = yc;
      }
      {
        qb = (qb - 1u);
      }
    }
  }
}

[[max_total_threads_per_threadgroup(64)]]
kernel void gpu_lexer_b(uint3 ce [[thread_position_in_grid]], const constant ve* F [[buffer(0)]], const device tint_array<we, 1>* N [[buffer(1)]], device tint_array<uint2, 1>* G [[buffer(2)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.F=F, .N=N, .G=G, .tint_immediate_data=tint_immediate_data};
  b_inner(ce, tint_module_vars);
}

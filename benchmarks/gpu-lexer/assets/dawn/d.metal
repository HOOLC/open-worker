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
  device tint_array<float, 1>* l;
  threadgroup tint_array<float4, 1024>* j;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

struct tint_array_lengths_struct {
  uint tint_array_length_0_1;
  uint tint_array_length_0_2;
};

struct tint_symbol_1 {
  tint_array<float4, 1024> tint_symbol;
};

uint X(uint3 Yb) {
  return (Yb.x + (Yb.y * 65535u));
}

void d_inner(uint3 fe, uint3 I, uint tint_local_index, tint_module_vars_struct tint_module_vars) {
  {
    uint v_1 = 0u;
    v_1 = tint_local_index;
    while(true) {
      uint const v_2 = v_1;
      if ((v_2 >= 1024u)) {
        break;
      }
      (*tint_module_vars.j)[v_2] = float4(0.0f);
      {
        v_1 = (v_2 + 256u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  tint_array_lengths_struct const v_3 = tint_array_lengths_struct{.tint_array_length_0_1=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].y / 24u), .tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 4u)};
  uint const Gc = X(fe);
  uint const Hc = (Gc >> (2u & 31u));
  if ((Hc >= (*tint_module_vars.F).a)) {
    return;
  }
  we const fa = (*tint_module_vars.N)[min(Hc, (v_3.tint_array_length_0_1 - 1u))];
  uint const wb = (I.x & 31u);
  uint const Ic = (((Gc & 3u) * 8u) + (I.x >> (5u & 31u)));
  float xb = 0.0f;
  float yb = 0.0f;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint zb = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((zb < fa.c)) {
      } else {
        break;
      }
      uint const Ab = (zb + wb);
      uint const ge = ((fa.c - 1u) - Ab);
      bool const Jc = (Ab < fa.c);
      uint const Bb = ((((fa.e + Ab) * 32u) + Ic) * 4u);
      uint const Cb = ((((fa.e + ge) * 32u) + Ic) * 4u);
      float4 x = float4(1.0f, 0.0f, 1.0f, 0.0f);
      if (Jc) {
        x = float4((*tint_module_vars.l)[min(Bb, (v_3.tint_array_length_0_2 - 1u))], (*tint_module_vars.l)[min((Bb + 1u), (v_3.tint_array_length_0_2 - 1u))], (*tint_module_vars.l)[min((Cb + 2u), (v_3.tint_array_length_0_2 - 1u))], (*tint_module_vars.l)[min((Cb + 3u), (v_3.tint_array_length_0_2 - 1u))]);
      }
      (*tint_module_vars.j)[I.x] = x;
      {
        uint2 tint_loop_idx_1 = uint2(4294967295u);
        uint ga = 1u;
        while(true) {
          if (all((tint_loop_idx_1 == uint2(0u)))) {
            break;
          }
          if ((ga < 32u)) {
          } else {
            break;
          }
          threadgroup_barrier(mem_flags::mem_threadgroup);
          float4 ha = float4(1.0f, 0.0f, 1.0f, 0.0f);
          if ((wb >= ga)) {
            ha = (*tint_module_vars.j)[min((I.x - ga), 1023u)];
          }
          threadgroup_barrier(mem_flags::mem_threadgroup);
          x = float4((x.x * ha.x), ((x.x * ha.y) + x.y), (x.z * ha.z), ((x.z * ha.w) + x.w));
          (*tint_module_vars.j)[I.x] = x;
          {
            uint const tint_low_inc_1 = (tint_loop_idx_1.x - 1u);
            tint_loop_idx_1.x = tint_low_inc_1;
            uint const tint_carry_1 = uint((tint_low_inc_1 == 4294967295u));
            tint_loop_idx_1.y = (tint_loop_idx_1.y - tint_carry_1);
            ga = (ga << (1u & 31u));
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      float4 ia = float4(1.0f, 0.0f, 1.0f, 0.0f);
      if ((wb > 0u)) {
        ia = (*tint_module_vars.j)[min((I.x - 1u), 1023u)];
      }
      if (Jc) {
        (*tint_module_vars.l)[min((Bb + 1u), (v_3.tint_array_length_0_2 - 1u))] = ((ia.x * xb) + ia.y);
        (*tint_module_vars.l)[min((Cb + 3u), (v_3.tint_array_length_0_2 - 1u))] = ((ia.z * yb) + ia.w);
      }
      float4 const Ga = (*tint_module_vars.j)[min((I.x | 31u), 1023u)];
      xb = ((Ga.x * xb) + Ga.y);
      yb = ((Ga.z * yb) + Ga.w);
      threadgroup_barrier(mem_flags::mem_threadgroup);
      {
        uint const tint_low_inc = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc;
        uint const tint_carry = uint((tint_low_inc == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
        zb = (zb + 32u);
      }
    }
  }
}

[[max_total_threads_per_threadgroup(256)]]
kernel void gpu_lexer_d(uint3 fe [[threadgroup_position_in_grid]], uint3 I [[thread_position_in_threadgroup]], uint tint_local_index [[thread_index_in_threadgroup]], const constant ve* F [[buffer(0)]], const device tint_array<we, 1>* N [[buffer(1)]], device tint_array<float, 1>* l [[buffer(2)]], threadgroup tint_symbol_1* v_4 [[threadgroup(0)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.F=F, .N=N, .l=l, .j=(&(*v_4).tint_symbol), .tint_immediate_data=tint_immediate_data};
  d_inner(fe, I, tint_local_index, tint_module_vars);
}

/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

struct tint_array_lengths_struct {
  uint tint_array_length_0_0;
  uint tint_array_length_0_3;
  uint tint_array_length_0_4;
  uint tint_array_length_0_2;
  uint tint_array_length_0_7;
  uint tint_array_length_0_5;
  uint tint_array_length_0_6;
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

struct ve {
  /* 0x0000 */ uint a;
  /* 0x0004 */ uint b;
  /* 0x0008 */ uint c;
  /* 0x000c */ uint d;
};

struct we {
  /* 0x0000 */ uint start;
  /* 0x0004 */ uint count;
  /* 0x0008 */ uint e;
  /* 0x000c */ uint c;
  /* 0x0010 */ uint f;
  /* 0x0014 */ uint g;
};

struct xe {
  /* 0x0000 */ uint start;
  /* 0x0004 */ uint count;
  /* 0x0008 */ uint stream;
  /* 0x000c */ uint h;
};

struct tint_immediate_data_struct {
  /* 0x0000 */ tint_array<uint4, 2> tint_storage_buffer_sizes;
};

struct tint_module_vars_struct {
  const device tint_array<uint, 1>* M;
  const constant ve* F;
  const device tint_array<we, 1>* N;
  const device tint_array<float, 1>* i;
  const device tint_array<xe, 1>* va;
  device tint_array<float, 1>* ib;
  device tint_array<float, 1>* l;
  device tint_array<uint2, 1>* G;
  threadgroup tint_array<float4, 1024>* j;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

struct tint_symbol_1 {
  tint_array<float4, 1024> tint_symbol;
};

uint X(uint3 Yb) {
  return (Yb.x + (Yb.y * 65535u));
}

float fb(float Gd) {
  return (1.0f / (1.0f + exp(-(Gd))));
}

float gb(uint wa, uint Hd, uint Id, uint t, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  bool v_1 = false;
  if ((wa < Hd)) {
    v_1 = true;
  } else {
    v_1 = (wa >= Id);
  }
  if (v_1) {
    return 0.0f;
  }
  uint const Y = (*tint_module_vars.M)[min((wa * 2u), (tint_array_lengths.tint_array_length_0_0 - 1u))];
  uint const O = (*tint_module_vars.M)[min(((wa * 2u) + 1u), (tint_array_lengths.tint_array_length_0_0 - 1u))];
  uint const ac = (Y & 3u);
  float u = (*tint_module_vars.i)[min(((0u + (ac * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))];
  u = (u + (*tint_module_vars.i)[min(((0u + ((4u + ((Y >> (2u & 31u)) & 7u)) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  u = (u + (*tint_module_vars.i)[min(((0u + ((12u + ((Y >> (5u & 31u)) & 127u)) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  u = (u + (*tint_module_vars.i)[min(((0u + ((140u + ((Y >> (12u & 31u)) & 127u)) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  if ((ac == 0u)) {
    u = (u + (*tint_module_vars.i)[min(((0u + ((268u + (((Y >> (19u & 31u)) & 255u) & 255u)) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
    u = (u + (*tint_module_vars.i)[min(((0u + ((524u + ((O >> (15u & 31u)) & 127u)) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  }
  uint const Jd = 652u;
  uint const Kd = (O & 127u);
  {
    uint xa = 0u;
    while(true) {
      if ((xa < 7u)) {
      } else {
        break;
      }
      if (((Kd & (1u << (xa & 31u))) != 0u)) {
        u = (u + (*tint_module_vars.i)[min(((0u + ((Jd + xa) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
      }
      {
        xa = (xa + 1u);
      }
    }
  }
  uint const bc = ((O >> (7u & 31u)) & 15u);
  uint const cc = ((O >> (11u & 31u)) & 15u);
  if ((bc != 0u)) {
    u = (u + (*tint_module_vars.i)[min(((0u + (((659u + bc) - 1u) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  }
  if ((cc != 0u)) {
    u = (u + (*tint_module_vars.i)[min(((0u + (((673u + cc) - 1u) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  }
  uint const dc = ((O >> (22u & 31u)) & 31u);
  uint const ec = ((O >> (27u & 31u)) & 31u);
  if ((dc != 0u)) {
    u = (u + (*tint_module_vars.i)[min(((0u + (((687u + dc) - 1u) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  }
  if ((ec != 0u)) {
    u = (u + (*tint_module_vars.i)[min(((0u + (((718u + ec) - 1u) * 32u)) + t), (tint_array_lengths.tint_array_length_0_3 - 1u))]);
  }
  return u;
}

float Ed(uint kb, uint H, uint ya, uint za, float2 fc, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  float Aa = (*tint_module_vars.i)[min((23968u + H), (tint_array_lengths.tint_array_length_0_3 - 1u))];
  {
    uint P = 0u;
    while(true) {
      if ((P < 5u)) {
      } else {
        break;
      }
      bool v_2 = false;
      if (((kb + P) >= (ya + 2u))) {
        v_2 = ((kb + P) < (za + 2u));
      } else {
        v_2 = false;
      }
      if (v_2) {
        float const v_3 = Aa;
        float const v_4 = gb(((kb + P) - 2u), ya, za, H, tint_array_lengths, tint_module_vars);
        Aa = (v_3 + (v_4 * (*tint_module_vars.i)[min(((36905u + (P * 32u)) + H), (tint_array_lengths.tint_array_length_0_3 - 1u))]));
      }
      {
        P = (P + 1u);
      }
    }
  }
  float const v_5 = Aa;
  float const v_6 = gb(as_type<uint>(fc.x), ya, za, H, tint_array_lengths, tint_module_vars);
  Aa = (v_5 + (v_6 * (*tint_module_vars.i)[min((37065u + H), (tint_array_lengths.tint_array_length_0_3 - 1u))]));
  float const v_7 = Aa;
  float const v_8 = gb(as_type<uint>(fc.y), ya, za, H, tint_array_lengths, tint_module_vars);
  Aa = (v_7 + (v_8 * (*tint_module_vars.i)[min((37097u + H), (tint_array_lengths.tint_array_length_0_3 - 1u))]));
  return tanh(Aa);
}

float2 Vb(uint Ld, uint Ba, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  float gc = (*tint_module_vars.i)[min((38153u + Ba), (tint_array_lengths.tint_array_length_0_3 - 1u))];
  float hc = (*tint_module_vars.i)[min((39209u + Ba), (tint_array_lengths.tint_array_length_0_3 - 1u))];
  {
    uint Z = 0u;
    while(true) {
      if ((Z < 32u)) {
      } else {
        break;
      }
      float const ic = (*tint_module_vars.j)[min(((Ld * 32u) + Z), 1023u)].x;
      gc = (gc + (ic * (*tint_module_vars.i)[min(((37129u + (Ba * 32u)) + Z), (tint_array_lengths.tint_array_length_0_3 - 1u))]));
      hc = (hc + (ic * (*tint_module_vars.i)[min(((38185u + (Ba * 32u)) + Z), (tint_array_lengths.tint_array_length_0_3 - 1u))]));
      {
        Z = (Z + 1u);
      }
    }
  }
  float const jc = fb(hc);
  return float2(jc, ((1.0f - jc) * tanh(gc)));
}

void c_inner(uint3 de, uint3 ee, uint tint_local_index, tint_module_vars_struct tint_module_vars) {
  {
    uint v_9 = 0u;
    v_9 = tint_local_index;
    while(true) {
      uint const v_10 = v_9;
      if ((v_10 >= 1024u)) {
        break;
      }
      (*tint_module_vars.j)[v_10] = float4(0.0f);
      {
        v_9 = (v_10 + 32u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  tint_array_lengths_struct const v_11 = tint_array_lengths_struct{.tint_array_length_0_0=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].x / 4u), .tint_array_length_0_3=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].w / 4u), .tint_array_length_0_4=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].x / 16u), .tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 24u), .tint_array_length_0_7=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].w / 8u), .tint_array_length_0_5=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].y / 4u), .tint_array_length_0_6=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].z / 4u)};
  uint const ba = X(de);
  if ((ba >= (*tint_module_vars.F).c)) {
    return;
  }
  xe const s = (*tint_module_vars.va)[min(ba, (v_11.tint_array_length_0_4 - 1u))];
  we const rb = (*tint_module_vars.N)[min(s.stream, (v_11.tint_array_length_0_2 - 1u))];
  uint const q = ee.x;
  if ((q < s.count)) {
    uint const zc = (s.start + q);
    uint Ac = (*tint_module_vars.G)[min(ba, (v_11.tint_array_length_0_7 - 1u))].y;
    uint Bc = (*tint_module_vars.G)[min(ba, (v_11.tint_array_length_0_7 - 1u))].x;
    {
      uint2 tint_loop_idx = uint2(4294967295u);
      uint Da = zc;
      while(true) {
        if (all((tint_loop_idx == uint2(0u)))) {
          break;
        }
        if ((Da > s.start)) {
        } else {
          break;
        }
        uint const Cc = ((*tint_module_vars.M)[min(((Da - 1u) * 2u), (v_11.tint_array_length_0_0 - 1u))] & 3u);
        bool v_12 = false;
        if ((Cc != 1u)) {
          v_12 = (Cc != 2u);
        } else {
          v_12 = false;
        }
        if (v_12) {
          Ac = (Da - 1u);
          break;
        }
        {
          uint const tint_low_inc = (tint_loop_idx.x - 1u);
          tint_loop_idx.x = tint_low_inc;
          uint const tint_carry = uint((tint_low_inc == 4294967295u));
          tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
          Da = (Da - 1u);
        }
      }
    }
    {
      uint2 tint_loop_idx = uint2(4294967295u);
      uint Ea = (zc + 1u);
      while(true) {
        if (all((tint_loop_idx == uint2(0u)))) {
          break;
        }
        if ((Ea < (s.start + s.count))) {
        } else {
          break;
        }
        uint const Dc = ((*tint_module_vars.M)[min((Ea * 2u), (v_11.tint_array_length_0_0 - 1u))] & 3u);
        bool v_13 = false;
        if ((Dc != 1u)) {
          v_13 = (Dc != 2u);
        } else {
          v_13 = false;
        }
        if (v_13) {
          Bc = Ea;
          break;
        }
        {
          uint const tint_low_inc_1 = (tint_loop_idx.x - 1u);
          tint_loop_idx.x = tint_low_inc_1;
          uint const tint_carry_1 = uint((tint_low_inc_1 == 4294967295u));
          tint_loop_idx.y = (tint_loop_idx.y - tint_carry_1);
          Ea = (Ea + 1u);
        }
      }
    }
    (*tint_module_vars.j)[(q * 32u)].y = as_type<float>(Ac);
    (*tint_module_vars.j)[(q * 32u)].z = as_type<float>(Bc);
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint Q = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((Q < s.count)) {
      } else {
        break;
      }
      float const Ec = Ed((s.start + Q), q, rb.start, (rb.start + rb.count), (*tint_module_vars.j)[min((Q * 32u), 1023u)].yz, v_11, tint_module_vars);
      (*tint_module_vars.ib)[min((((s.start + Q) * 32u) + q), (v_11.tint_array_length_0_5 - 1u))] = Ec;
      (*tint_module_vars.j)[min(((Q * 32u) + q), 1023u)].x = Ec;
      {
        uint const tint_low_inc_2 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_2;
        uint const tint_carry_2 = uint((tint_low_inc_2 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_2);
        Q = (Q + 1u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint ca = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((ca < s.count)) {
      } else {
        break;
      }
      float2 const Fc = Vb(ca, q, v_11, tint_module_vars);
      (*tint_module_vars.j)[min(((ca * 32u) + q), 1023u)].y = Fc.x;
      (*tint_module_vars.j)[min(((ca * 32u) + q), 1023u)].z = Fc.y;
      {
        uint const tint_low_inc_3 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_3;
        uint const tint_carry_3 = uint((tint_low_inc_3 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_3);
        ca = (ca + 1u);
      }
    }
  }
  float2 da = float2(1.0f, 0.0f);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint sb = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((sb < s.count)) {
      } else {
        break;
      }
      float4 const tb = (*tint_module_vars.j)[min(((sb * 32u) + q), 1023u)];
      da = float2((tb.y * da.x), ((tb.y * da.y) + tb.z));
      {
        uint const tint_low_inc_4 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_4;
        uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_4);
        sb = (sb + 1u);
      }
    }
  }
  float2 ea = float2(1.0f, 0.0f);
  {
    uint ub = s.count;
    while(true) {
      if ((ub > 0u)) {
      } else {
        break;
      }
      float4 const vb = (*tint_module_vars.j)[min((((ub - 1u) * 32u) + q), 1023u)];
      ea = float2((vb.y * ea.x), ((vb.y * ea.y) + vb.z));
      {
        ub = (ub - 1u);
      }
    }
  }
  uint const Fa = (((ba * 32u) + q) * 4u);
  (*tint_module_vars.l)[min(Fa, (v_11.tint_array_length_0_6 - 1u))] = da.x;
  (*tint_module_vars.l)[min((Fa + 1u), (v_11.tint_array_length_0_6 - 1u))] = da.y;
  (*tint_module_vars.l)[min((Fa + 2u), (v_11.tint_array_length_0_6 - 1u))] = ea.x;
  (*tint_module_vars.l)[min((Fa + 3u), (v_11.tint_array_length_0_6 - 1u))] = ea.y;
}

[[max_total_threads_per_threadgroup(32)]]
kernel void gpu_lexer_c(uint3 de [[threadgroup_position_in_grid]], uint3 ee [[thread_position_in_threadgroup]], uint tint_local_index [[thread_index_in_threadgroup]], const device tint_array<uint, 1>* M [[buffer(0)]], const constant ve* F [[buffer(1)]], const device tint_array<we, 1>* N [[buffer(2)]], const device tint_array<float, 1>* i [[buffer(3)]], const device tint_array<xe, 1>* va [[buffer(4)]], device tint_array<float, 1>* ib [[buffer(5)]], device tint_array<float, 1>* l [[buffer(6)]], device tint_array<uint2, 1>* G [[buffer(7)]], threadgroup tint_symbol_1* v_14 [[threadgroup(0)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.M=M, .F=F, .N=N, .i=i, .va=va, .ib=ib, .l=l, .G=G, .j=(&(*v_14).tint_symbol), .tint_immediate_data=tint_immediate_data};
  c_inner(de, ee, tint_local_index, tint_module_vars);
}

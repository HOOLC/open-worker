/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

struct tint_array_lengths_struct {
  uint tint_array_length_0_2;
  uint tint_array_length_0_5;
  uint tint_array_length_0_1;
  uint tint_array_length_0_6;
  uint tint_array_length_0_7;
  uint tint_array_length_0_3;
  uint tint_array_length_0_4;
};

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
  const constant ve* F;
  const device tint_array<we, 1>* N;
  const device tint_array<float, 1>* i;
  device tint_array<half, 1>* ua;
  device tint_array<half, 1>* r;
  const device tint_array<xe, 1>* va;
  device tint_array<float, 1>* ib;
  device tint_array<float, 1>* l;
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

float2 Vb(uint Ld, uint Ba, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  float gc = (*tint_module_vars.i)[min((38153u + Ba), (tint_array_lengths.tint_array_length_0_2 - 1u))];
  float hc = (*tint_module_vars.i)[min((39209u + Ba), (tint_array_lengths.tint_array_length_0_2 - 1u))];
  {
    uint Z = 0u;
    while(true) {
      if ((Z < 32u)) {
      } else {
        break;
      }
      float const ic = (*tint_module_vars.j)[min(((Ld * 32u) + Z), 1023u)].x;
      gc = (gc + (ic * (*tint_module_vars.i)[min(((37129u + (Ba * 32u)) + Z), (tint_array_lengths.tint_array_length_0_2 - 1u))]));
      hc = (hc + (ic * (*tint_module_vars.i)[min(((38185u + (Ba * 32u)) + Z), (tint_array_lengths.tint_array_length_0_2 - 1u))]));
      {
        Z = (Z + 1u);
      }
    }
  }
  float const jc = fb(hc);
  return float2(jc, ((1.0f - jc) * tanh(gc)));
}

float hb(float kc, float lc, float Md, float Nd, uint mc, uint Od, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  uint const aa = ((min(Od, 11u) * 32u) + mc);
  float const Pd = tanh((((((kc * (*tint_module_vars.i)[min((24000u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))]) + (lc * (*tint_module_vars.i)[min((24384u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Md * (*tint_module_vars.i)[min((24768u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Nd * (*tint_module_vars.i)[min((25152u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (*tint_module_vars.i)[min((25536u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))]));
  float const Qd = select(lc, kc, (mc < 16u));
  return ((Pd + Qd) * 0.5f);
}

uint tint_div_u32(uint lhs, uint rhs) {
  return (lhs / select(rhs, 1u, (rhs == 0u)));
}

void e_inner(uint3 he, uint3 ie, uint tint_local_index, tint_module_vars_struct tint_module_vars) {
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
        v_1 = (v_2 + 32u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  tint_array_lengths_struct const v_3 = tint_array_lengths_struct{.tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 4u), .tint_array_length_0_5=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].y / 16u), .tint_array_length_0_1=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].y / 24u), .tint_array_length_0_6=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].z / 4u), .tint_array_length_0_7=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].w / 4u), .tint_array_length_0_3=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].w / 2u), .tint_array_length_0_4=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].x / 2u)};
  uint const Db = X(he);
  if ((Db >= (*tint_module_vars.F).c)) {
    return;
  }
  xe const y = (*tint_module_vars.va)[min(Db, (v_3.tint_array_length_0_5 - 1u))];
  we const Kc = (*tint_module_vars.N)[min(y.stream, (v_3.tint_array_length_0_1 - 1u))];
  uint const k = ie.x;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint Ha = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((Ha < y.count)) {
      } else {
        break;
      }
      (*tint_module_vars.j)[min(((Ha * 32u) + k), 1023u)].x = (*tint_module_vars.ib)[min((((y.start + Ha) * 32u) + k), (v_3.tint_array_length_0_6 - 1u))];
      {
        uint const tint_low_inc = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc;
        uint const tint_carry = uint((tint_low_inc == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
        Ha = (Ha + 1u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint ja = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((ja < y.count)) {
      } else {
        break;
      }
      float2 const Lc = Vb(ja, k, v_3, tint_module_vars);
      (*tint_module_vars.j)[min(((ja * 32u) + k), 1023u)].y = Lc.x;
      (*tint_module_vars.j)[min(((ja * 32u) + k), 1023u)].z = Lc.y;
      {
        uint const tint_low_inc_1 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_1;
        uint const tint_carry_1 = uint((tint_low_inc_1 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_1);
        ja = (ja + 1u);
      }
    }
  }
  uint const Mc = (((Db * 32u) + k) * 4u);
  float J = (*tint_module_vars.l)[min((Mc + 1u), (v_3.tint_array_length_0_7 - 1u))];
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint Eb = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((Eb < y.count)) {
      } else {
        break;
      }
      uint const Nc = ((Eb * 32u) + k);
      float4 const Oc = (*tint_module_vars.j)[min(Nc, 1023u)];
      J = ((Oc.y * J) + Oc.z);
      (*tint_module_vars.j)[min(Nc, 1023u)].w = J;
      {
        uint const tint_low_inc_2 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_2;
        uint const tint_carry_2 = uint((tint_low_inc_2 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_2);
        Eb = (Eb + 1u);
      }
    }
  }
  J = (*tint_module_vars.l)[min((Mc + 3u), (v_3.tint_array_length_0_7 - 1u))];
  {
    uint Fb = y.count;
    while(true) {
      if ((Fb > 0u)) {
      } else {
        break;
      }
      uint const Pc = (((Fb - 1u) * 32u) + k);
      float4 const Qc = (*tint_module_vars.j)[min(Pc, 1023u)];
      J = ((Qc.y * J) + Qc.z);
      (*tint_module_vars.j)[min(Pc, 1023u)].z = J;
      {
        Fb = (Fb - 1u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint ka = 0u;
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((ka < y.count)) {
      } else {
        break;
      }
      uint const Rc = (y.start + ka);
      float Sc = ((*tint_module_vars.ib)[min(((Rc * 32u) + k), (v_3.tint_array_length_0_6 - 1u))] + (*tint_module_vars.i)[min((41289u + k), (v_3.tint_array_length_0_2 - 1u))]);
      uint const Tc = (39241u + (k * 64u));
      {
        uint la = 0u;
        while(true) {
          if ((la < 32u)) {
          } else {
            break;
          }
          float4 const Uc = (*tint_module_vars.j)[min(((ka * 32u) + la), 1023u)];
          Sc = (Sc + ((Uc.w * (*tint_module_vars.i)[min((Tc + la), (v_3.tint_array_length_0_2 - 1u))]) + (Uc.z * (*tint_module_vars.i)[min(((Tc + 32u) + la), (v_3.tint_array_length_0_2 - 1u))])));
          {
            la = (la + 1u);
          }
        }
      }
      float const Vc = float(half(tanh(Sc)));
      (*tint_module_vars.j)[min(((ka * 32u) + k), 1023u)].y = Vc;
      (*tint_module_vars.ua)[min(((Rc * 32u) + k), (v_3.tint_array_length_0_3 - 1u))] = half(Vc);
      {
        uint const tint_low_inc_3 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_3;
        uint const tint_carry_3 = uint((tint_low_inc_3 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_3);
        ka = (ka + 1u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  uint Gb = 32u;
  uint Ia = y.count;
  uint ma = 0u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((Gb <= 1u)) {
        break;
      }
      uint const Wc = tint_div_u32(Gb, 2u);
      {
        uint2 tint_loop_idx_1 = uint2(4294967295u);
        uint na = 0u;
        while(true) {
          if (all((tint_loop_idx_1 == uint2(0u)))) {
            break;
          }
          if ((na < Wc)) {
          } else {
            break;
          }
          uint const Ja = (na * 2u);
          float Ka = 0.0f;
          if ((Ja < Ia)) {
            uint const La = (Ja * 32u);
            bool const Ma = ((ma & 1u) == 0u);
            float const Xc = select((*tint_module_vars.j)[min((La + k), 1023u)].x, (*tint_module_vars.j)[min((La + k), 1023u)].y, Ma);
            Ka = Xc;
            if (((Ja + 1u) < Ia)) {
              uint const Na = ((Ja + 1u) * 32u);
              uint const Oa = (k ^ (1u << (ma & 31u)));
              Ka = hb(Xc, select((*tint_module_vars.j)[min((Na + k), 1023u)].x, (*tint_module_vars.j)[min((Na + k), 1023u)].y, Ma), select((*tint_module_vars.j)[min((La + Oa), 1023u)].x, (*tint_module_vars.j)[min((La + Oa), 1023u)].y, Ma), select((*tint_module_vars.j)[min((Na + Oa), 1023u)].x, (*tint_module_vars.j)[min((Na + Oa), 1023u)].y, Ma), k, ma, v_3, tint_module_vars);
            }
          }
          if (((ma & 1u) == 0u)) {
            (*tint_module_vars.j)[min(((na * 32u) + k), 1023u)].x = Ka;
          } else {
            (*tint_module_vars.j)[min(((na * 32u) + k), 1023u)].y = Ka;
          }
          {
            uint const tint_low_inc_5 = (tint_loop_idx_1.x - 1u);
            tint_loop_idx_1.x = tint_low_inc_5;
            uint const tint_carry_5 = uint((tint_low_inc_5 == 4294967295u));
            tint_loop_idx_1.y = (tint_loop_idx_1.y - tint_carry_5);
            na = (na + 1u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      Gb = Wc;
      Ia = tint_div_u32((Ia + 1u), 2u);
      ma = (ma + 1u);
      {
        uint const tint_low_inc_4 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_4;
        uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_4);
      }
    }
  }
  uint const je = (((Kc.f + Kc.g) - 1u) + y.h);
  (*tint_module_vars.r)[min(((je * 32u) + k), (v_3.tint_array_length_0_4 - 1u))] = half((*tint_module_vars.j)[k].x);
}

[[max_total_threads_per_threadgroup(32)]]
kernel void gpu_lexer_e(uint3 he [[threadgroup_position_in_grid]], uint3 ie [[thread_position_in_threadgroup]], uint tint_local_index [[thread_index_in_threadgroup]], const constant ve* F [[buffer(0)]], const device tint_array<we, 1>* N [[buffer(1)]], const device tint_array<float, 1>* i [[buffer(2)]], device tint_array<half, 1>* ua [[buffer(3)]], device tint_array<half, 1>* r [[buffer(4)]], const device tint_array<xe, 1>* va [[buffer(5)]], device tint_array<float, 1>* ib [[buffer(6)]], device tint_array<float, 1>* l [[buffer(7)]], threadgroup tint_symbol_1* v_4 [[threadgroup(0)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.F=F, .N=N, .i=i, .ua=ua, .r=r, .va=va, .ib=ib, .l=l, .j=(&(*v_4).tint_symbol), .tint_immediate_data=tint_immediate_data};
  e_inner(he, ie, tint_local_index, tint_module_vars);
}

/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

struct tint_array_lengths_struct {
  uint tint_array_length_0_2;
  uint tint_array_length_0_1;
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

struct tint_immediate_data_struct {
  /* 0x0000 */ tint_array<uint4, 2> tint_storage_buffer_sizes;
};

struct tint_module_vars_struct {
  const constant ve* F;
  const device tint_array<we, 1>* N;
  const device tint_array<float, 1>* i;
  device tint_array<half, 1>* r;
  device tint_array<float, 1>* l;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

uint X(uint3 Yb) {
  return (Yb.x + (Yb.y * 65535u));
}

float fb(float Gd) {
  return (1.0f / (1.0f + exp(-(Gd))));
}

float hb(float kc, float lc, float Md, float Nd, uint mc, uint Od, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  uint const aa = ((min(Od, 11u) * 32u) + mc);
  float const Pd = tanh((((((kc * (*tint_module_vars.i)[min((24000u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))]) + (lc * (*tint_module_vars.i)[min((24384u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Md * (*tint_module_vars.i)[min((24768u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Nd * (*tint_module_vars.i)[min((25152u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (*tint_module_vars.i)[min((25536u + aa), (tint_array_lengths.tint_array_length_0_2 - 1u))]));
  float const Qd = select(lc, kc, (mc < 16u));
  return ((Pd + Qd) * 0.5f);
}

float ta(float nc, float Rd, float Sd, float Td, float Ud, float Vd, uint Wd, uint Xd, bool Yd, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  uint const A = ((min(Xd, 11u) * 32u) + Wd);
  float const Zd = select((*tint_module_vars.i)[min((28872u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))], (*tint_module_vars.i)[min((29256u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))], Yd);
  float const ae = tanh((((((((nc * (*tint_module_vars.i)[min((35472u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))]) + (Rd * (*tint_module_vars.i)[min((26568u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Sd * (*tint_module_vars.i)[min((26952u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Td * (*tint_module_vars.i)[min((27336u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Ud * (*tint_module_vars.i)[min((27720u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + (Vd * (*tint_module_vars.i)[min((28104u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))])) + Zd));
  float const oc = fb((*tint_module_vars.i)[min((28488u + A), (tint_array_lengths.tint_array_length_0_2 - 1u))]);
  return ((nc * oc) + (ae * (1.0f - oc)));
}

uint tint_div_u32(uint lhs, uint rhs) {
  return (lhs / select(rhs, 1u, (rhs == 0u)));
}

void f_inner(uint3 ke, uint3 Yc, tint_module_vars_struct tint_module_vars) {
  tint_array_lengths_struct const v_1 = tint_array_lengths_struct{.tint_array_length_0_2=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].z / 4u), .tint_array_length_0_1=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].y / 24u), .tint_array_length_0_3=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].w / 2u), .tint_array_length_0_4=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].x / 4u)};
  uint const Zc = X(ke);
  uint const o = (Yc.x & 31u);
  uint const Pa = (Yc.x >> (5u & 31u));
  if ((Zc >= (*tint_module_vars.F).a)) {
    return;
  }
  we const p = (*tint_module_vars.N)[min(Zc, (v_1.tint_array_length_0_1 - 1u))];
  uint const le = ((p.f + p.g) - 1u);
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    uint Hb = (p.c + Pa);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((Hb < p.g)) {
      } else {
        break;
      }
      (*tint_module_vars.r)[min((((le + Hb) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))] = 0.0h;
      {
        uint const tint_low_inc = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc;
        uint const tint_carry = uint((tint_low_inc == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
        Hb = (Hb + 8u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_device);
  threadgroup_barrier(mem_flags::mem_threadgroup);
  uint v = p.g;
  uint Qa = p.c;
  uint Ib = 5u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((v <= 1u)) {
        break;
      }
      uint const Jb = tint_div_u32(v, 2u);
      uint const me = ((p.f + Jb) - 1u);
      uint const Ra = ((p.f + v) - 1u);
      uint const ad = (o ^ (1u << (min(Ib, 4u) & 31u)));
      {
        uint2 tint_loop_idx_1 = uint2(4294967295u);
        uint Sa = Pa;
        while(true) {
          if (all((tint_loop_idx_1 == uint2(0u)))) {
            break;
          }
          if ((Sa < Jb)) {
          } else {
            break;
          }
          uint const R = (Sa * 2u);
          float Kb = 0.0f;
          if ((R < Qa)) {
            float const bd = float((*tint_module_vars.r)[min((((Ra + R) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))]);
            Kb = bd;
            if (((R + 1u) < Qa)) {
              float const v_2 = float((*tint_module_vars.r)[min(((((Ra + R) + 1u) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))]);
              float const v_3 = float((*tint_module_vars.r)[min((((Ra + R) * 32u) + ad), (v_1.tint_array_length_0_3 - 1u))]);
              float const v_4 = float((*tint_module_vars.r)[min(((((Ra + R) + 1u) * 32u) + ad), (v_1.tint_array_length_0_3 - 1u))]);
              Kb = hb(bd, v_2, v_3, v_4, o, Ib, v_1, tint_module_vars);
            }
          }
          device half* const v_5 = (&(*tint_module_vars.r)[min((((me + Sa) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))]);
          (*v_5) = half(Kb);
          {
            uint const tint_low_inc_2 = (tint_loop_idx_1.x - 1u);
            tint_loop_idx_1.x = tint_low_inc_2;
            uint const tint_carry_2 = uint((tint_low_inc_2 == 4294967295u));
            tint_loop_idx_1.y = (tint_loop_idx_1.y - tint_carry_2);
            Sa = (Sa + 8u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_device);
      threadgroup_barrier(mem_flags::mem_threadgroup);
      v = Jb;
      Qa = tint_div_u32((Qa + 1u), 2u);
      Ib = (Ib + 1u);
      {
        uint const tint_low_inc_1 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_1;
        uint const tint_carry_1 = uint((tint_low_inc_1 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_1);
      }
    }
  }
  if ((Pa == 0u)) {
    (*tint_module_vars.l)[min(((p.f * 32u) + o), (v_1.tint_array_length_0_4 - 1u))] = float(half(float((*tint_module_vars.r)[min(((p.f * 32u) + o), (v_1.tint_array_length_0_3 - 1u))])));
  }
  threadgroup_barrier(mem_flags::mem_device);
  threadgroup_barrier(mem_flags::mem_threadgroup);
  v = 1u;
  uint S = (4u + (31u - clz(p.g)));
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((v >= p.g)) {
        break;
      }
      uint const cd = ((p.f + v) - 1u);
      uint const K = ((p.f + (v * 2u)) - 1u);
      uint const dd = tint_div_u32(p.g, (v * 2u));
      uint const ed = tint_div_u32(((p.c + dd) - 1u), dd);
      {
        uint2 tint_loop_idx_2 = uint2(4294967295u);
        uint oa = Pa;
        while(true) {
          if (all((tint_loop_idx_2 == uint2(0u)))) {
            break;
          }
          if ((oa < v)) {
          } else {
            break;
          }
          uint const B = (oa * 2u);
          if ((B >= ed)) {
            {
              uint const tint_low_inc_4 = (tint_loop_idx_2.x - 1u);
              tint_loop_idx_2.x = tint_low_inc_4;
              uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
              tint_loop_idx_2.y = (tint_loop_idx_2.y - tint_carry_4);
              oa = (oa + 8u);
            }
            continue;
          }
          float const Lb = (*tint_module_vars.l)[min((((cd + oa) * 32u) + o), (v_1.tint_array_length_0_4 - 1u))];
          float const fd = float((*tint_module_vars.r)[min((((K + B) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))]);
          if (((B + 1u) < ed)) {
            float const gd = float((*tint_module_vars.r)[min(((((K + B) + 1u) * 32u) + o), (v_1.tint_array_length_0_3 - 1u))]);
            uint const Mb = (o ^ (1u << (min(S, 4u) & 31u)));
            float const hd = (*tint_module_vars.l)[min((((cd + oa) * 32u) + Mb), (v_1.tint_array_length_0_4 - 1u))];
            float const id = float((*tint_module_vars.r)[min((((K + B) * 32u) + Mb), (v_1.tint_array_length_0_3 - 1u))]);
            float const jd = float((*tint_module_vars.r)[min(((((K + B) + 1u) * 32u) + Mb), (v_1.tint_array_length_0_3 - 1u))]);
            (*tint_module_vars.l)[min((((K + B) * 32u) + o), (v_1.tint_array_length_0_4 - 1u))] = float(half(ta(Lb, fd, gd, hd, id, jd, o, S, false, v_1, tint_module_vars)));
            (*tint_module_vars.l)[min(((((K + B) + 1u) * 32u) + o), (v_1.tint_array_length_0_4 - 1u))] = float(half(ta(Lb, gd, fd, hd, jd, id, o, S, true, v_1, tint_module_vars)));
          } else {
            (*tint_module_vars.l)[min((((K + B) * 32u) + o), (v_1.tint_array_length_0_4 - 1u))] = float(half(Lb));
          }
          {
            uint const tint_low_inc_4 = (tint_loop_idx_2.x - 1u);
            tint_loop_idx_2.x = tint_low_inc_4;
            uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
            tint_loop_idx_2.y = (tint_loop_idx_2.y - tint_carry_4);
            oa = (oa + 8u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_device);
      threadgroup_barrier(mem_flags::mem_threadgroup);
      v = (v * 2u);
      S = select(0u, (S - 1u), (S > 0u));
      {
        uint const tint_low_inc_3 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_3;
        uint const tint_carry_3 = uint((tint_low_inc_3 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_3);
      }
    }
  }
}

[[max_total_threads_per_threadgroup(256)]]
kernel void dawn_entry_point_687474703a2f2f3132372e302e302e3120687474703a2f2f3132372e302e302e31(uint3 ke [[threadgroup_position_in_grid]], uint3 Yc [[thread_position_in_threadgroup]], const constant ve* F [[buffer(0)]], const device tint_array<we, 1>* N [[buffer(1)]], const device tint_array<float, 1>* i [[buffer(2)]], device tint_array<half, 1>* r [[buffer(3)]], device tint_array<float, 1>* l [[buffer(4)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.F=F, .N=N, .i=i, .r=r, .l=l, .tint_immediate_data=tint_immediate_data};
  f_inner(ke, Yc, tint_module_vars);
}

/* Dumped generated MSL */
#ifdef __clang__
#pragma clang diagnostic ignored "-Wall"
#endif

#pragma METAL fp math_mode(relaxed)
#include <metal_stdlib>
using namespace metal;

struct tint_array_lengths_struct {
  uint tint_array_length_0_4;
  uint tint_array_length_0_6;
  uint tint_array_length_0_3;
  uint tint_array_length_0_5;
  uint tint_array_length_0_7;
  uint tint_array_length_0_0;
  uint tint_array_length_0_1;
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
  device tint_array<atomic_uint, 1>* Fd;
  const constant ve* F;
  const device tint_array<we, 1>* N;
  const device tint_array<float, 1>* i;
  device tint_array<half, 1>* ua;
  const device tint_array<xe, 1>* va;
  device tint_array<float, 1>* l;
  threadgroup tint_array<float, 2016>* m;
  threadgroup tint_array<float, 128>* Wb;
  threadgroup tint_array<float, 576>* Xb;
  threadgroup tint_array<float, 72>* jb;
  const constant tint_immediate_data_struct* tint_immediate_data;
};

struct tint_symbol_4 {
  tint_array<float, 2016> tint_symbol;
  tint_array<float, 128> tint_symbol_1;
  tint_array<float, 576> tint_symbol_2;
  tint_array<float, 72> tint_symbol_3;
};

uint X(uint3 Yb) {
  return (Yb.x + (Yb.y * 65535u));
}

float fb(float Gd) {
  return (1.0f / (1.0f + exp(-(Gd))));
}

float hb(float kc, float lc, float Md, float Nd, uint mc, uint Od, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  uint const aa = ((min(Od, 11u) * 32u) + mc);
  float const Pd = tanh((((((kc * (*tint_module_vars.i)[min((24000u + aa), (tint_array_lengths.tint_array_length_0_4 - 1u))]) + (lc * (*tint_module_vars.i)[min((24384u + aa), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Md * (*tint_module_vars.i)[min((24768u + aa), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Nd * (*tint_module_vars.i)[min((25152u + aa), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (*tint_module_vars.i)[min((25536u + aa), (tint_array_lengths.tint_array_length_0_4 - 1u))]));
  float const Qd = select(lc, kc, (mc < 16u));
  return ((Pd + Qd) * 0.5f);
}

float ta(float nc, float Rd, float Sd, float Td, float Ud, float Vd, uint Wd, uint Xd, bool Yd, tint_array_lengths_struct tint_array_lengths, tint_module_vars_struct tint_module_vars) {
  uint const A = ((min(Xd, 11u) * 32u) + Wd);
  float const Zd = select((*tint_module_vars.i)[min((28872u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))], (*tint_module_vars.i)[min((29256u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))], Yd);
  float const ae = tanh((((((((nc * (*tint_module_vars.i)[min((35472u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))]) + (Rd * (*tint_module_vars.i)[min((26568u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Sd * (*tint_module_vars.i)[min((26952u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Td * (*tint_module_vars.i)[min((27336u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Ud * (*tint_module_vars.i)[min((27720u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + (Vd * (*tint_module_vars.i)[min((28104u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))])) + Zd));
  float const oc = fb((*tint_module_vars.i)[min((28488u + A), (tint_array_lengths.tint_array_length_0_4 - 1u))]);
  return ((nc * oc) + (ae * (1.0f - oc)));
}

uint tint_div_u32(uint lhs, uint rhs) {
  return (lhs / select(rhs, 1u, (rhs == 0u)));
}

uint tint_mod_u32(uint lhs, uint rhs) {
  return (lhs - ((lhs / select(rhs, 1u, (rhs == 0u))) * select(rhs, 1u, (rhs == 0u))));
}

void g_inner(uint3 ne, uint3 Ta, uint tint_local_index, tint_module_vars_struct tint_module_vars) {
  {
    uint v_1 = 0u;
    v_1 = tint_local_index;
    while(true) {
      uint const v_2 = v_1;
      if ((v_2 >= 72u)) {
        break;
      }
      (*tint_module_vars.jb)[v_2] = 0.0f;
      {
        v_1 = (v_2 + 64u);
      }
    }
  }
  {
    uint v_3 = 0u;
    v_3 = tint_local_index;
    while(true) {
      uint const v_4 = v_3;
      if ((v_4 >= 128u)) {
        break;
      }
      (*tint_module_vars.Wb)[v_4] = 0.0f;
      {
        v_3 = (v_4 + 64u);
      }
    }
  }
  {
    uint v_5 = 0u;
    v_5 = tint_local_index;
    while(true) {
      uint const v_6 = v_5;
      if ((v_6 >= 576u)) {
        break;
      }
      (*tint_module_vars.Xb)[v_6] = 0.0f;
      {
        v_5 = (v_6 + 64u);
      }
    }
  }
  {
    uint v_7 = 0u;
    v_7 = tint_local_index;
    while(true) {
      uint const v_8 = v_7;
      if ((v_8 >= 2016u)) {
        break;
      }
      (*tint_module_vars.m)[v_8] = 0.0f;
      {
        v_7 = (v_8 + 64u);
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  tint_array_lengths_struct const v_9 = tint_array_lengths_struct{.tint_array_length_0_4=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].x / 4u), .tint_array_length_0_6=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].z / 16u), .tint_array_length_0_3=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].w / 24u), .tint_array_length_0_5=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].y / 2u), .tint_array_length_0_7=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[1u].w / 4u), .tint_array_length_0_0=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].x / 4u), .tint_array_length_0_1=((*tint_module_vars.tint_immediate_data).tint_storage_buffer_sizes[0u].y / 4u)};
  uint const kd = X(ne);
  if ((kd >= (*tint_module_vars.F).c)) {
    return;
  }
  xe const D = (*tint_module_vars.va)[min(kd, (v_9.tint_array_length_0_6 - 1u))];
  we const ld = (*tint_module_vars.N)[min(D.stream, (v_9.tint_array_length_0_3 - 1u))];
  bool const Ua = (Ta.x < 32u);
  uint const n = Ta.x;
  if (Ua) {
    {
      uint2 tint_loop_idx = uint2(4294967295u);
      uint Va = 0u;
      while(true) {
        if (all((tint_loop_idx == uint2(0u)))) {
          break;
        }
        if ((Va < D.count)) {
        } else {
          break;
        }
        threadgroup float* const v_10 = (&(*tint_module_vars.m)[min((((31u + Va) * 32u) + n), 2015u)]);
        (*v_10) = float((*tint_module_vars.ua)[min((((D.start + Va) * 32u) + n), (v_9.tint_array_length_0_5 - 1u))]);
        {
          uint const tint_low_inc = (tint_loop_idx.x - 1u);
          tint_loop_idx.x = tint_low_inc;
          uint const tint_carry = uint((tint_low_inc == 4294967295u));
          tint_loop_idx.y = (tint_loop_idx.y - tint_carry);
          Va = (Va + 1u);
        }
      }
    }
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  uint w = 32u;
  uint Wa = D.count;
  uint z = 0u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((w <= 1u)) {
        break;
      }
      uint const Nb = tint_div_u32(w, 2u);
      uint const oe = (Nb - 1u);
      uint const Xa = (w - 1u);
      if (Ua) {
        {
          uint2 tint_loop_idx_1 = uint2(4294967295u);
          uint Ya = 0u;
          while(true) {
            if (all((tint_loop_idx_1 == uint2(0u)))) {
              break;
            }
            if ((Ya < Nb)) {
            } else {
              break;
            }
            uint const T = (Ya * 2u);
            float Ob = 0.0f;
            if ((T < Wa)) {
              float const md = (*tint_module_vars.m)[min((((Xa + T) * 32u) + n), 2015u)];
              Ob = md;
              if (((T + 1u) < Wa)) {
                uint const nd = (n ^ (1u << (z & 31u)));
                Ob = hb(md, (*tint_module_vars.m)[min(((((Xa + T) + 1u) * 32u) + n), 2015u)], (*tint_module_vars.m)[min((((Xa + T) * 32u) + nd), 2015u)], (*tint_module_vars.m)[min(((((Xa + T) + 1u) * 32u) + nd), 2015u)], n, z, v_9, tint_module_vars);
              }
            }
            (*tint_module_vars.m)[min((((oe + Ya) * 32u) + n), 2015u)] = Ob;
            {
              uint const tint_low_inc_2 = (tint_loop_idx_1.x - 1u);
              tint_loop_idx_1.x = tint_low_inc_2;
              uint const tint_carry_2 = uint((tint_low_inc_2 == 4294967295u));
              tint_loop_idx_1.y = (tint_loop_idx_1.y - tint_carry_2);
              Ya = (Ya + 1u);
            }
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      w = Nb;
      Wa = tint_div_u32((Wa + 1u), 2u);
      z = (z + 1u);
      {
        uint const tint_low_inc_1 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_1;
        uint const tint_carry_1 = uint((tint_low_inc_1 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_1);
      }
    }
  }
  if (Ua) {
    uint const pe = (((ld.f + ld.g) - 1u) + D.h);
    (*tint_module_vars.m)[n] = (*tint_module_vars.l)[min(((pe * 32u) + n), (v_9.tint_array_length_0_7 - 1u))];
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  w = 1u;
  z = 4u;
  {
    uint2 tint_loop_idx = uint2(4294967295u);
    while(true) {
      if (all((tint_loop_idx == uint2(0u)))) {
        break;
      }
      if ((w >= 32u)) {
        break;
      }
      uint const od = (w - 1u);
      uint const L = ((w * 2u) - 1u);
      uint const pd = tint_div_u32(32u, (w * 2u));
      uint const qd = tint_div_u32(((D.count + pd) - 1u), pd);
      if (Ua) {
        {
          uint2 tint_loop_idx_2 = uint2(4294967295u);
          uint pa = 0u;
          while(true) {
            if (all((tint_loop_idx_2 == uint2(0u)))) {
              break;
            }
            if ((pa < w)) {
            } else {
              break;
            }
            uint const C = (pa * 2u);
            if ((C >= qd)) {
              {
                uint const tint_low_inc_4 = (tint_loop_idx_2.x - 1u);
                tint_loop_idx_2.x = tint_low_inc_4;
                uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
                tint_loop_idx_2.y = (tint_loop_idx_2.y - tint_carry_4);
                pa = (pa + 1u);
              }
              continue;
            }
            float const Pb = (*tint_module_vars.m)[min((((od + pa) * 32u) + n), 2015u)];
            float const rd = (*tint_module_vars.m)[min((((L + C) * 32u) + n), 2015u)];
            if (((C + 1u) < qd)) {
              float const sd = (*tint_module_vars.m)[min(((((L + C) + 1u) * 32u) + n), 2015u)];
              uint const Qb = (n ^ (1u << (z & 31u)));
              float const td = (*tint_module_vars.m)[min((((od + pa) * 32u) + Qb), 2015u)];
              float const ud = (*tint_module_vars.m)[min((((L + C) * 32u) + Qb), 2015u)];
              float const vd = (*tint_module_vars.m)[min(((((L + C) + 1u) * 32u) + Qb), 2015u)];
              (*tint_module_vars.m)[min((((L + C) * 32u) + n), 2015u)] = ta(Pb, rd, sd, td, ud, vd, n, z, false, v_9, tint_module_vars);
              (*tint_module_vars.m)[min(((((L + C) + 1u) * 32u) + n), 2015u)] = ta(Pb, sd, rd, td, vd, ud, n, z, true, v_9, tint_module_vars);
            } else {
              (*tint_module_vars.m)[min((((L + C) * 32u) + n), 2015u)] = Pb;
            }
            {
              uint const tint_low_inc_4 = (tint_loop_idx_2.x - 1u);
              tint_loop_idx_2.x = tint_low_inc_4;
              uint const tint_carry_4 = uint((tint_low_inc_4 == 4294967295u));
              tint_loop_idx_2.y = (tint_loop_idx_2.y - tint_carry_4);
              pa = (pa + 1u);
            }
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      w = (w * 2u);
      z = select(0u, (z - 1u), (z > 0u));
      {
        uint const tint_low_inc_3 = (tint_loop_idx.x - 1u);
        tint_loop_idx.x = tint_low_inc_3;
        uint const tint_carry_3 = uint((tint_low_inc_3 == 4294967295u));
        tint_loop_idx.y = (tint_loop_idx.y - tint_carry_3);
      }
    }
  }
  uint const E = tint_div_u32(Ta.x, 8u);
  uint const Za = tint_mod_u32(Ta.x, 8u);
  {
    uint Rb = 0u;
    while(true) {
      if ((Rb < 4u)) {
      } else {
        break;
      }
      uint const ab = ((Rb * 8u) + E);
      uint const qa = (D.start + ab);
      bool const Sb = (ab < D.count);
      uint const wd = select(1u, ((*tint_module_vars.M)[min((qa * 2u), (v_9.tint_array_length_0_0 - 1u))] & 3u), Sb);
      bool v_11 = false;
      if (Sb) {
        v_11 = (wd != 1u);
      } else {
        v_11 = false;
      }
      bool v_12 = false;
      if (v_11) {
        v_12 = (wd != 2u);
      } else {
        v_12 = false;
      }
      bool const bb = v_12;
      {
        uint2 tint_loop_idx = uint2(4294967295u);
        uint ra = Za;
        while(true) {
          if (all((tint_loop_idx == uint2(0u)))) {
            break;
          }
          bool v_13 = false;
          if (bb) {
            v_13 = (ra < 16u);
          } else {
            v_13 = false;
          }
          if (v_13) {
          } else {
            break;
          }
          float xd = (*tint_module_vars.i)[min((36889u + ra), (v_9.tint_array_length_0_4 - 1u))];
          {
            uint U = 0u;
            while(true) {
              if ((U < 32u)) {
              } else {
                break;
              }
              float const qe = float((*tint_module_vars.ua)[min(((qa * 32u) + U), (v_9.tint_array_length_0_5 - 1u))]);
              float const re = float(half((*tint_module_vars.m)[(((31u + ab) * 32u) + U)]));
              uint const yd = (35865u + (ra * 64u));
              xd = (xd + (((*tint_module_vars.i)[min((yd + U), (v_9.tint_array_length_0_4 - 1u))] * qe) + ((*tint_module_vars.i)[min(((yd + 32u) + U), (v_9.tint_array_length_0_4 - 1u))] * re)));
              {
                U = (U + 1u);
              }
            }
          }
          threadgroup float* const v_14 = (&(*tint_module_vars.Wb)[min(((E * 16u) + ra), 127u)]);
          (*v_14) = fb(xd);
          {
            uint const tint_low_inc_5 = (tint_loop_idx.x - 1u);
            tint_loop_idx.x = tint_low_inc_5;
            uint const tint_carry_5 = uint((tint_low_inc_5 == 4294967295u));
            tint_loop_idx.y = (tint_loop_idx.y - tint_carry_5);
            ra = (ra + 8u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      {
        uint2 tint_loop_idx = uint2(4294967295u);
        uint V = Za;
        while(true) {
          if (all((tint_loop_idx == uint2(0u)))) {
            break;
          }
          bool v_15 = false;
          if (bb) {
            v_15 = (V < 72u);
          } else {
            v_15 = false;
          }
          if (v_15) {
          } else {
            break;
          }
          float Tb = (*tint_module_vars.i)[min((35400u + V), (v_9.tint_array_length_0_4 - 1u))];
          {
            uint W = 0u;
            while(true) {
              if ((W < 32u)) {
              } else {
                break;
              }
              float const se = float((*tint_module_vars.ua)[min(((qa * 32u) + W), (v_9.tint_array_length_0_5 - 1u))]);
              float const te = float(half((*tint_module_vars.m)[(((31u + ab) * 32u) + W)]));
              uint const zd = (29640u + (V * 80u));
              Tb = (Tb + (((*tint_module_vars.i)[min((zd + W), (v_9.tint_array_length_0_4 - 1u))] * se) + ((*tint_module_vars.i)[min(((zd + 32u) + W), (v_9.tint_array_length_0_4 - 1u))] * te)));
              {
                W = (W + 1u);
              }
            }
          }
          uint const ue = ((29640u + (V * 80u)) + 64u);
          {
            uint cb = 0u;
            while(true) {
              if ((cb < 16u)) {
              } else {
                break;
              }
              Tb = (Tb + ((*tint_module_vars.i)[min((ue + cb), (v_9.tint_array_length_0_4 - 1u))] * (*tint_module_vars.Wb)[((E * 16u) + cb)]));
              {
                cb = (cb + 1u);
              }
            }
          }
          (*tint_module_vars.Xb)[min(((E * 72u) + V), 575u)] = tanh(Tb);
          {
            uint const tint_low_inc_6 = (tint_loop_idx.x - 1u);
            tint_loop_idx.x = tint_low_inc_6;
            uint const tint_carry_6 = uint((tint_low_inc_6 == 4294967295u));
            tint_loop_idx.y = (tint_loop_idx.y - tint_carry_6);
            V = (V + 8u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      {
        uint2 tint_loop_idx = uint2(4294967295u);
        uint sa = Za;
        while(true) {
          if (all((tint_loop_idx == uint2(0u)))) {
            break;
          }
          bool v_16 = false;
          if (bb) {
            v_16 = (sa < 9u);
          } else {
            v_16 = false;
          }
          if (v_16) {
          } else {
            break;
          }
          float Ad = (*tint_module_vars.i)[min((35856u + sa), (v_9.tint_array_length_0_4 - 1u))];
          {
            uint db = 0u;
            while(true) {
              if ((db < 72u)) {
              } else {
                break;
              }
              Ad = (Ad + ((*tint_module_vars.i)[min(((25920u + (sa * 72u)) + db), (v_9.tint_array_length_0_4 - 1u))] * (*tint_module_vars.Xb)[((E * 72u) + db)]));
              {
                db = (db + 1u);
              }
            }
          }
          (*tint_module_vars.jb)[min(((E * 9u) + sa), 71u)] = Ad;
          {
            uint const tint_low_inc_7 = (tint_loop_idx.x - 1u);
            tint_loop_idx.x = tint_low_inc_7;
            uint const tint_carry_7 = uint((tint_low_inc_7 == 4294967295u));
            tint_loop_idx.y = (tint_loop_idx.y - tint_carry_7);
            sa = (sa + 8u);
          }
        }
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      bool v_17 = false;
      if ((Za == 0u)) {
        v_17 = Sb;
      } else {
        v_17 = false;
      }
      if (v_17) {
        uint Bd = 0u;
        if (bb) {
          float Cd = (*tint_module_vars.jb)[(E * 9u)];
          {
            uint eb = 1u;
            while(true) {
              if ((eb < 9u)) {
              } else {
                break;
              }
              float const Dd = (*tint_module_vars.jb)[((E * 9u) + eb)];
              if ((Dd > Cd)) {
                Cd = Dd;
                Bd = eb;
              }
              {
                eb = (eb + 1u);
              }
            }
          }
        }
        device atomic_uint* const v_18 = (&(*tint_module_vars.Fd)[min(tint_div_u32(qa, 4u), (v_9.tint_array_length_0_1 - 1u))]);
        uint const ignored = atomic_fetch_or_explicit(v_18, (Bd << (((qa & 3u) * 8u) & 31u)), memory_order_relaxed);
      }
      threadgroup_barrier(mem_flags::mem_threadgroup);
      {
        Rb = (Rb + 1u);
      }
    }
  }
}

[[max_total_threads_per_threadgroup(64)]]
kernel void dawn_entry_point_687474703a2f2f3132372e302e302e3120687474703a2f2f3132372e302e302e31(uint3 ne [[threadgroup_position_in_grid]], uint3 Ta [[thread_position_in_threadgroup]], uint tint_local_index [[thread_index_in_threadgroup]], const device tint_array<uint, 1>* M [[buffer(0)]], device tint_array<atomic_uint, 1>* Fd [[buffer(1)]], const constant ve* F [[buffer(2)]], const device tint_array<we, 1>* N [[buffer(3)]], const device tint_array<float, 1>* i [[buffer(4)]], device tint_array<half, 1>* ua [[buffer(5)]], const device tint_array<xe, 1>* va [[buffer(6)]], device tint_array<float, 1>* l [[buffer(7)]], threadgroup tint_symbol_4* v_19 [[threadgroup(0)]], const constant tint_immediate_data_struct* tint_immediate_data [[buffer(30)]]) {
  tint_module_vars_struct const tint_module_vars = tint_module_vars_struct{.M=M, .Fd=Fd, .F=F, .N=N, .i=i, .ua=ua, .va=va, .l=l, .m=(&(*v_19).tint_symbol), .Wb=(&(*v_19).tint_symbol_1), .Xb=(&(*v_19).tint_symbol_2), .jb=(&(*v_19).tint_symbol_3), .tint_immediate_data=tint_immediate_data};
  g_inner(ne, Ta, tint_local_index, tint_module_vars);
}

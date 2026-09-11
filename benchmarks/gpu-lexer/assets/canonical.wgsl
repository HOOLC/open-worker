enable f16;struct ve{a:u32,b:u32,c:u32,d:u32}struct we{start:u32,count:u32,e:u32,c:u32,f:u32,g:u32}struct xe{start:u32,count:u32,stream:u32,h:u32}@group(0) @binding(0) var<storage,read> M:array<u32>;@group(0) @binding(1) var<storage,read_write> Fd:array<atomic<u32>>;@group(0) @binding(2) var<uniform> F:ve;@group(0) @binding(3) var<storage,read> N:array<we>;@group(0) @binding(4) var<storage,read> i:array<f32>;@group(0) @binding(5) var<storage,read_write> ua:array<f16>;@group(0) @binding(6) var<storage,read_write> r:array<f16>;@group(0) @binding(9) var<storage,read> va:array<xe>;@group(0) @binding(10) var<storage,read_write> ib:array<f32>;@group(0) @binding(11) var<storage,read_write> l:array<f32>;@group(0) @binding(12) var<storage,read_write> G:array<vec2<u32>>;var<workgroup> j:array<vec4<f32>,1024>;var<workgroup> m:array<f32,2016>;var<workgroup> Wb:array<f32,128>;var<workgroup> Xb:array<f32,576>;var<workgroup> jb:array<f32,72>;fn X(Yb:vec3<u32>)->u32{return Yb.x+Yb.y*65535u;}fn Ub(Zb:vec3<u32>)->u32{return Zb.x+Zb.y*4194240u;}fn fb(Gd:f32)->f32{return 1./(1.+exp(-Gd));}fn gb(wa:u32,Hd:u32,Id:u32,t:u32)->f32{if (wa<Hd||wa>=Id){return 0.;}let Y=M[wa*2u];let O=M[wa*2u+1u];let ac=Y&3u;var u=i[0u+ac*32u+t];u+=i[0u+(4u+((Y>>2u)&7u))*32u+t];u+=i[0u+(12u+((Y>>5u)&127u))*32u+t];u+=i[0u+(140u+((Y>>12u)&127u))*32u+t];if (ac==0u){u+=i[0u+(268u+(((Y>>19u)&255u)&255u))*32u+t];u+=i[0u+(524u+((O>>15u)&127u))*32u+t];}let Jd=652u;let Kd=O&127u;{var xa=0u;loop{if(!(xa<7u)){break;}if ((Kd&(1u<<xa))!=0u){u+=i[0u+(Jd+xa)*32u+t];}xa++;}}let bc=(O>>7u)&15u;let cc=(O>>11u)&15u;if (bc!=0u){u+=i[0u+(659u+bc-1u)*32u+t];}if (cc!=0u){u+=i[0u+(673u+cc-1u)*32u+t];}let dc=(O>>22u)&31u;let ec=(O>>27u)&31u;if (dc!=0u){u+=i[0u+(687u+dc-1u)*32u+t];}if (ec!=0u){u+=i[0u+(718u+ec-1u)*32u+t];}return u;}fn Ed(kb:u32,H:u32,ya:u32,za:u32,fc:vec2<f32>)->f32{var Aa=i[23968u+H];{var P=0u;loop{if(!(P<5u)){break;}if (kb+P>=ya+2u&&kb+P<za+2u){Aa+=gb(kb+P-2u,ya,za,H)*i[36905u+P*32u+H];}P++;}}Aa+=gb(bitcast<u32>(fc.x),ya,za,H)*i[37065u+H];Aa+=gb(bitcast<u32>(fc.y),ya,za,H)*i[37097u+H];return tanh(Aa);}fn Vb(Ld:u32,Ba:u32)->vec2<f32> {var gc=i[38153u+Ba];var hc=i[39209u+Ba];{var Z=0u;loop{if(!(Z<32u)){break;}let ic=j[Ld*32u+Z].x;gc+=ic*i[37129u+Ba*32u+Z];hc+=ic*i[38185u+Ba*32u+Z];Z++;}}let jc=fb(hc);return vec2<f32>(jc,(1.-jc)*tanh(gc));}fn hb(kc:f32,lc:f32,Md:f32,Nd:f32,mc:u32,Od:u32)->f32{let aa=min(Od,11u)*32u+mc;let Pd=tanh(kc*i[24000u+aa]+lc*i[24384u+aa]+Md*i[24768u+aa]+Nd*i[25152u+aa]+i[25536u+aa]);let Qd=select(lc,kc,mc<16u);return (Pd+Qd)*.5;}fn ta(nc:f32,Rd:f32,Sd:f32,Td:f32,Ud:f32,Vd:f32,Wd:u32,Xd:u32,Yd:bool)->f32{let A=min(Xd,11u)*32u+Wd;let Zd=select(i[28872u+A],i[29256u+A],Yd);let ae=tanh(nc*i[35472u+A]+Rd*i[26568u+A]+Sd*i[26952u+A]+Td*i[27336u+A]+Ud*i[27720u+A]+Vd*i[28104u+A]+Zd);let oc=fb(i[28488u+A]);return nc*oc+ae*(1.-oc);}@compute @workgroup_size(64) fn a(@builtin(global_invocation_id) be:vec3<u32>){let lb=Ub(be);if (lb>=F.c){return;}let pc=va[lb];var mb=0xffffffffu;var qc=0xffffffffu;{var nb=0u;loop{if(!(nb<pc.count)){break;}let ob=pc.start+nb;let rc=M[ob*2u]&3u;if (rc!=1u&&rc!=2u){if (mb==0xffffffffu){mb=ob;}qc=ob;}nb++;}}G[lb]=vec2<u32>(mb,qc);}@compute @workgroup_size(64) fn b(@builtin(global_invocation_id) ce:vec3<u32>){let sc=Ub(ce);if (sc>=F.a){return;}let Ca=N[sc];var tc=0xffffffffu;{var pb=0u;loop{if(!(pb<Ca.c)){break;}let uc=Ca.e+pb;let vc=G[uc];G[uc].y=tc;if (vc.y!=0xffffffffu){tc=vc.y;}pb++;}}var wc=0xffffffffu;{var qb=Ca.c;loop{if(!(qb>0u)){break;}let xc=Ca.e+qb-1u;let yc=G[xc].x;G[xc].x=wc;if (yc!=0xffffffffu){wc=yc;}qb--;}}}@compute @workgroup_size(32) fn c(@builtin(workgroup_id) de:vec3<u32>,@builtin(local_invocation_id) ee:vec3<u32>){let ba=X(de);if (ba>=F.c){return;}let s=va[ba];let rb=N[s.stream];let q=ee.x;if (q<s.count){let zc=s.start+q;var Ac=G[ba].y;var Bc=G[ba].x;{var Da=zc;loop{if(!(Da>s.start)){break;}let Cc=M[(Da-1u)*2u]&3u;if (Cc!=1u&&Cc!=2u){Ac=Da-1u;break;}Da--;}}{var Ea=zc+1u;loop{if(!(Ea<s.start+s.count)){break;}let Dc=M[Ea*2u]&3u;if (Dc!=1u&&Dc!=2u){Bc=Ea;break;}Ea++;}}j[q*32u].y=bitcast<f32>(Ac);j[q*32u].z=bitcast<f32>(Bc);}workgroupBarrier();{var Q=0u;loop{if(!(Q<s.count)){break;}let Ec=Ed(s.start+Q,q,rb.start,rb.start+rb.count,j[Q*32u].yz);ib[(s.start+Q)*32u+q]=Ec;j[Q*32u+q].x=Ec;Q++;}}workgroupBarrier();{var ca=0u;loop{if(!(ca<s.count)){break;}let Fc=Vb(ca,q);j[ca*32u+q].y=Fc.x;j[ca*32u+q].z=Fc.y;ca++;}}var da=vec2<f32>(1.,0.);{var sb=0u;loop{if(!(sb<s.count)){break;}let tb=j[sb*32u+q];da=vec2<f32>(tb.y*da.x,tb.y*da.y+tb.z);sb++;}}var ea=vec2<f32>(1.,0.);{var ub=s.count;loop{if(!(ub>0u)){break;}let vb=j[(ub-1u)*32u+q];ea=vec2<f32>(vb.y*ea.x,vb.y*ea.y+vb.z);ub--;}}let Fa=(ba*32u+q)*4u;l[Fa]=da.x;l[Fa+1u]=da.y;l[Fa+2u]=ea.x;l[Fa+3u]=ea.y;}@compute @workgroup_size(256) fn d(@builtin(workgroup_id) fe:vec3<u32>,@builtin(local_invocation_id) I:vec3<u32>){let Gc=X(fe);let Hc=Gc>>2u;if (Hc>=F.a){return;}let fa=N[Hc];let wb=I.x&31u;let Ic=(Gc&3u)*8u+(I.x>>5u);var xb=0.;var yb=0.;{var zb=0u;loop{if(!(zb<fa.c)){break;}let Ab=zb+wb;let ge=fa.c-1u-Ab;let Jc=Ab<fa.c;let Bb=((fa.e+Ab)*32u+Ic)*4u;let Cb=((fa.e+ge)*32u+Ic)*4u;var x=vec4<f32>(1.,0.,1.,0.);if (Jc){x=vec4<f32>(l[Bb],l[Bb+1u],l[Cb+2u],l[Cb+3u]);}j[I.x]=x;{var ga=1u;loop{if(!(ga<32u)){break;}workgroupBarrier();var ha=vec4<f32>(1.,0.,1.,0.);if (wb>=ga){ha=j[I.x-ga];}workgroupBarrier();x=vec4<f32>(x.x*ha.x,x.x*ha.y+x.y,x.z*ha.z,x.z*ha.w+x.w);j[I.x]=x;ga=ga<<1u;}}workgroupBarrier();var ia=vec4<f32>(1.,0.,1.,0.);if (wb>0u){ia=j[I.x-1u];}if (Jc){l[Bb+1u]=ia.x*xb+ia.y;l[Cb+3u]=ia.z*yb+ia.w;}let Ga=j[I.x|31u];xb=Ga.x*xb+Ga.y;yb=Ga.z*yb+Ga.w;workgroupBarrier();zb+=32u;}}}@compute @workgroup_size(32) fn e(@builtin(workgroup_id) he:vec3<u32>,@builtin(local_invocation_id) ie:vec3<u32>){let Db=X(he);if (Db>=F.c){return;}let y=va[Db];let Kc=N[y.stream];let k=ie.x;{var Ha=0u;loop{if(!(Ha<y.count)){break;}j[Ha*32u+k].x=ib[(y.start+Ha)*32u+k];Ha++;}}workgroupBarrier();{var ja=0u;loop{if(!(ja<y.count)){break;}let Lc=Vb(ja,k);j[ja*32u+k].y=Lc.x;j[ja*32u+k].z=Lc.y;ja++;}}let Mc=(Db*32u+k)*4u;var J=l[Mc+1u];{var Eb=0u;loop{if(!(Eb<y.count)){break;}let Nc=Eb*32u+k;let Oc=j[Nc];J=Oc.y*J+Oc.z;j[Nc].w=J;Eb++;}}J=l[Mc+3u];{var Fb=y.count;loop{if(!(Fb>0u)){break;}let Pc=(Fb-1u)*32u+k;let Qc=j[Pc];J=Qc.y*J+Qc.z;j[Pc].z=J;Fb--;}}workgroupBarrier();{var ka=0u;loop{if(!(ka<y.count)){break;}let Rc=y.start+ka;var Sc=ib[Rc*32u+k]+i[41289u+k];let Tc=39241u+k*64u;{var la=0u;loop{if(!(la<32u)){break;}let Uc=j[ka*32u+la];Sc+=Uc.w*i[Tc+la]+Uc.z*i[Tc+32u+la];la++;}}let Vc=f32(f16(tanh(Sc)));j[ka*32u+k].y=Vc;ua[Rc*32u+k]=f16(Vc);ka++;}}workgroupBarrier();var Gb=32u;var Ia=y.count;var ma=0u;loop{if (Gb<=1u){break;}let Wc=Gb/2u;{var na=0u;loop{if(!(na<Wc)){break;}let Ja=na*2u;var Ka=0.;if (Ja<Ia){let La=Ja*32u;let Ma=(ma&1u)==0u;let Xc=select(j[La+k].x,j[La+k].y,Ma);Ka=Xc;if (Ja+1u<Ia){let Na=(Ja+1u)*32u;let Oa=k^(1u<<ma);Ka=hb(Xc,select(j[Na+k].x,j[Na+k].y,Ma),select(j[La+Oa].x,j[La+Oa].y,Ma),select(j[Na+Oa].x,j[Na+Oa].y,Ma),k,ma);}}if ((ma&1u)==0u){j[na*32u+k].x=Ka;} else{j[na*32u+k].y=Ka;}na++;}}workgroupBarrier();Gb=Wc;Ia=(Ia+1u)/2u;ma+=1u;}let je=Kc.f+Kc.g-1u+y.h;r[je*32u+k]=f16(j[k].x);}@compute @workgroup_size(256) fn f(@builtin(workgroup_id) ke:vec3<u32>,@builtin(local_invocation_id) Yc:vec3<u32>){let Zc=X(ke);let o=Yc.x&31u;let Pa=Yc.x>>5u;if (Zc>=F.a){return;}let p=N[Zc];let le=p.f+p.g-1u;{var Hb=p.c+Pa;loop{if(!(Hb<p.g)){break;}r[(le+Hb)*32u+o]=f16(0.);Hb+=8u;}}storageBarrier();workgroupBarrier();var v=p.g;var Qa=p.c;var Ib=5u;loop{if (v<=1u){break;}let Jb=v/2u;let me=p.f+Jb-1u;let Ra=p.f+v-1u;let ad=o^(1u<<min(Ib,4u));{var Sa=Pa;loop{if(!(Sa<Jb)){break;}let R=Sa*2u;var Kb=0.;if (R<Qa){let bd=f32(r[(Ra+R)*32u+o]);Kb=bd;if (R+1u<Qa){Kb=hb(bd,f32(r[(Ra+R+1u)*32u+o]),f32(r[(Ra+R)*32u+ad]),f32(r[(Ra+R+1u)*32u+ad]),o,Ib);}}r[(me+Sa)*32u+o]=f16(Kb);Sa+=8u;}}storageBarrier();workgroupBarrier();v=Jb;Qa=(Qa+1u)/2u;Ib+=1u;}if (Pa==0u){l[p.f*32u+o]=f32(f16(f32(r[p.f*32u+o])));}storageBarrier();workgroupBarrier();v=1u;var S=4u+(31u-countLeadingZeros(p.g));loop{if (v>=p.g){break;}let cd=p.f+v-1u;let K=p.f+v*2u-1u;let dd=p.g/(v*2u);let ed=(p.c+dd-1u)/dd;for(var oa=Pa;oa<v;oa+=8u){let B=oa*2u;if (B>=ed){continue;}let Lb=l[(cd+oa)*32u+o];let fd=f32(r[(K+B)*32u+o]);if (B+1u<ed){let gd=f32(r[(K+B+1u)*32u+o]);let Mb=o^(1u<<min(S,4u));let hd=l[(cd+oa)*32u+Mb];let id=f32(r[(K+B)*32u+Mb]);let jd=f32(r[(K+B+1u)*32u+Mb]);l[(K+B)*32u+o]=f32(f16(ta(Lb,fd,gd,hd,id,jd,o,S,false)));l[(K+B+1u)*32u+o]=f32(f16(ta(Lb,gd,fd,hd,jd,id,o,S,true)));} else{l[(K+B)*32u+o]=f32(f16(Lb));}}storageBarrier();workgroupBarrier();v*=2u;S=select(0u,S-1u,S>0u);}}@compute @workgroup_size(64) fn g(@builtin(workgroup_id) ne:vec3<u32>,@builtin(local_invocation_id) Ta:vec3<u32>){let kd=X(ne);if (kd>=F.c){return;}let D=va[kd];let ld=N[D.stream];let Ua=Ta.x<32u;let n=Ta.x;if (Ua){{var Va=0u;loop{if(!(Va<D.count)){break;}m[(31u+Va)*32u+n]=f32(ua[(D.start+Va)*32u+n]);Va++;}}}workgroupBarrier();var w=32u;var Wa=D.count;var z=0u;loop{if (w<=1u){break;}let Nb=w/2u;let oe=Nb-1u;let Xa=w-1u;if (Ua){{var Ya=0u;loop{if(!(Ya<Nb)){break;}let T=Ya*2u;var Ob=0.;if (T<Wa){let md=m[(Xa+T)*32u+n];Ob=md;if (T+1u<Wa){let nd=n^(1u<<z);Ob=hb(md,m[(Xa+T+1u)*32u+n],m[(Xa+T)*32u+nd],m[(Xa+T+1u)*32u+nd],n,z);}}m[(oe+Ya)*32u+n]=Ob;Ya++;}}}workgroupBarrier();w=Nb;Wa=(Wa+1u)/2u;z+=1u;}if (Ua){let pe=ld.f+ld.g-1u+D.h;m[n]=l[pe*32u+n];}workgroupBarrier();w=1u;z=4u;loop{if (w>=32u){break;}let od=w-1u;let L=w*2u-1u;let pd=32u/(w*2u);let qd=(D.count+pd-1u)/pd;if (Ua){for(var pa=0u;pa<w;pa++){let C=pa*2u;if (C>=qd){continue;}let Pb=m[(od+pa)*32u+n];let rd=m[(L+C)*32u+n];if (C+1u<qd){let sd=m[(L+C+1u)*32u+n];let Qb=n^(1u<<z);let td=m[(od+pa)*32u+Qb];let ud=m[(L+C)*32u+Qb];let vd=m[(L+C+1u)*32u+Qb];m[(L+C)*32u+n]=ta(Pb,rd,sd,td,ud,vd,n,z,false);m[(L+C+1u)*32u+n]=ta(Pb,sd,rd,td,vd,ud,n,z,true);} else{m[(L+C)*32u+n]=Pb;}}}workgroupBarrier();w*=2u;z=select(0u,z-1u,z>0u);}let E=Ta.x/8u;let Za=Ta.x%8u;{var Rb=0u;loop{if(!(Rb<4u)){break;}let ab=Rb*8u+E;let qa=D.start+ab;let Sb=ab<D.count;var wd=1u;if(Sb){wd=M[qa*2u]&3u;}let bb=Sb&&wd!=1u&&wd!=2u;{var ra=Za;loop{if(!(bb&&ra<16u)){break;}var xd=i[36889u+ra];{var U=0u;loop{if(!(U<32u)){break;}let qe=f32(ua[qa*32u+U]);let re=f32(f16(m[(31u+ab)*32u+U]));let yd=35865u+ra*64u;xd+=i[yd+U]*qe+i[yd+32u+U]*re;U++;}}Wb[E*16u+ra]=fb(xd);ra+=8u;}}workgroupBarrier();{var V=Za;loop{if(!(bb&&V<72u)){break;}var Tb=i[35400u+V];{var W=0u;loop{if(!(W<32u)){break;}let se=f32(ua[qa*32u+W]);let te=f32(f16(m[(31u+ab)*32u+W]));let zd=29640u+V*80u;Tb+=i[zd+W]*se+i[zd+32u+W]*te;W++;}}let ue=29640u+V*80u+64u;{var cb=0u;loop{if(!(cb<16u)){break;}Tb+=i[ue+cb]*Wb[E*16u+cb];cb++;}}Xb[E*72u+V]=tanh(Tb);V+=8u;}}workgroupBarrier();{var sa=Za;loop{if(!(bb&&sa<9u)){break;}var Ad=i[35856u+sa];{var db=0u;loop{if(!(db<72u)){break;}Ad+=i[25920u+sa*72u+db]*Xb[E*72u+db];db++;}}jb[E*9u+sa]=Ad;sa+=8u;}}workgroupBarrier();if (Za==0u&&Sb){var Bd=0u;if (bb){var Cd=jb[E*9u];{var eb=1u;loop{if(!(eb<9u)){break;}let Dd=jb[E*9u+eb];if (Dd>Cd){Cd=Dd;Bd=eb;}eb++;}}}let ignored=atomicOr(&Fd[qa/4u],Bd<<((qa&3u)*8u));}workgroupBarrier();Rb++;}}}
// Exact single-chunk specialization (1..32 tokens). One workgroup owns the
// entire stream, so global recurrent scans and duplicate tree reductions vanish.
var<workgroup> tiny_base:array<f32,1024>;
@compute @workgroup_size(32)
fn tiny(@builtin(local_invocation_id) tid:vec3<u32>) {
    let k=tid.x&31u;
    let count=min(F.b,32u);
    if(k<count) {
        var previous=0xffffffffu;
        var next=0xffffffffu;
        {var pos=k;loop{if(!(pos>0u)){break;}
            let kind=M[(pos-1u)*2u]&3u;
            if(kind!=1u && kind!=2u) {previous=pos-1u;break;}
        pos--;}}
        {var pos=k+1u;loop{if(!(pos<count)){break;}
            let kind=M[pos*2u]&3u;
            if(kind!=1u && kind!=2u) {next=pos;break;}
        pos++;}}
        j[k*32u].y=bitcast<f32>(previous);
        j[k*32u].z=bitcast<f32>(next);
    }
    workgroupBarrier();
    {var token=0u;loop{if(!(token<count)){break;}
        j[token*32u+k].x=Ed(token,k,0u,count,j[token*32u].yz);
    token++;}}
    workgroupBarrier();
    {var token=0u;loop{if(!(token<count)){break;}
        let gate=Vb(token,k);
        j[token*32u+k].y=gate.x;
        j[token*32u+k].z=gate.y;
    token++;}}
    // For a single chunk the scan prefix/suffix state is exactly zero.
    var state=0.;
    {var token=0u;loop{if(!(token<count)){break;}
        let at=token*32u+k;
        state=j[at].y*state+j[at].z;
        j[at].w=state;
    token++;}}
    state=0.;
    {var token=count;loop{if(!(token>0u)){break;}
        let at=(token-1u)*32u+k;
        state=j[at].y*state+j[at].z;
        j[at].z=state;
    token--;}}
    workgroupBarrier();
    {var token=0u;loop{if(!(token<count)){break;}
        var mixed=j[token*32u+k].x+i[41289u+k];
        let weight=39241u+k*64u;
        {var channel=0u;loop{if(!(channel<32u)){break;}
            let context=j[token*32u+channel];
            mixed+=context.w*i[weight+channel]+context.z*i[weight+32u+channel];
        channel++;}}
        let value=f32(f16(tanh(mixed)));
        tiny_base[token*32u+k]=value;
        m[(31u+token)*32u+k]=value;
    token++;}}
    workgroupBarrier();
    var capacity=32u;
    var valid_nodes=count;
    var level=0u;
    loop {
        if(capacity<=1u) {break;}
        let parents=capacity/2u;
        let parent_base=parents-1u;
        let child_base=capacity-1u;
        {var node=0u;loop{if(!(node<parents)){break;}
            let left=node*2u;
            var value=0.;
            if(left<valid_nodes) {
                let first=m[(child_base+left)*32u+k];
                value=first;
                if(left+1u<valid_nodes) {
                    let partner=k^(1u<<level);
                    value=hb(first,m[(child_base+left+1u)*32u+k],
                        m[(child_base+left)*32u+partner],m[(child_base+left+1u)*32u+partner],k,level);
                }
            }
            m[(parent_base+node)*32u+k]=value;
        node++;}}
        workgroupBarrier();
        capacity=parents;
        valid_nodes=(valid_nodes+1u)/2u;
        level+=1u;
    }
    // Match the upstream r:f16 -> l:f32 root round trip exactly.
    m[k]=f32(f16(m[k]));
    workgroupBarrier();
    capacity=1u;
    level=4u;
    loop {
        if(capacity>=32u) {break;}
        let parent_base=capacity-1u;
        let child_base=capacity*2u-1u;
        let per_child=32u/(capacity*2u);
        let active_children=(count+per_child-1u)/per_child;
        for(var node=0u;node<capacity;node++) {
            let left=node*2u;
            if(left>=active_children) {continue;}
            let parent=m[(parent_base+node)*32u+k];
            let first=m[(child_base+left)*32u+k];
            if(left+1u<active_children) {
                let second=m[(child_base+left+1u)*32u+k];
                let partner=k^(1u<<level);
                let parent_partner=m[(parent_base+node)*32u+partner];
                let first_partner=m[(child_base+left)*32u+partner];
                let second_partner=m[(child_base+left+1u)*32u+partner];
                m[(child_base+left)*32u+k]=ta(parent,first,second,parent_partner,first_partner,second_partner,k,level,false);
                m[(child_base+left+1u)*32u+k]=ta(parent,second,first,parent_partner,second_partner,first_partner,k,level,true);
            } else {m[(child_base+left)*32u+k]=parent;}
        }
        workgroupBarrier();
        capacity*=2u;
        level=select(0u,level-1u,level>0u);
    }
    // Four tokens in parallel, eight lanes per token, eight rounds.
    let token_lane=k/8u;
    let lane=k%8u;
    {var round=0u;loop{if(!(round<8u)){break;}
        let token=round*4u+token_lane;
        let valid=token<count;
        var kind=1u;
        if(valid) {kind=M[token*2u]&3u;}
        let classify=valid && kind!=1u && kind!=2u;
        {var gate=lane;loop{if(!(classify && gate<16u)){break;}
            var value=i[36889u+gate];
            {var channel=0u;loop{if(!(channel<32u)){break;}
                let base=tiny_base[token*32u+channel];
                let context=f32(f16(m[(31u+token)*32u+channel]));
                let weight=35865u+gate*64u;
                value+=i[weight+channel]*base+i[weight+32u+channel]*context;
            channel++;}}
            Wb[token_lane*16u+gate]=fb(value);
        gate+=8u;}}
        workgroupBarrier();
        {var hidden=lane;loop{if(!(classify && hidden<72u)){break;}
            var value=i[35400u+hidden];
            {var channel=0u;loop{if(!(channel<32u)){break;}
                let base=tiny_base[token*32u+channel];
                let context=f32(f16(m[(31u+token)*32u+channel]));
                let weight=29640u+hidden*80u;
                value+=i[weight+channel]*base+i[weight+32u+channel]*context;
            channel++;}}
            let extra=29640u+hidden*80u+64u;
            {var gate=0u;loop{if(!(gate<16u)){break;}value+=i[extra+gate]*Wb[token_lane*16u+gate];gate++;}}
            Xb[token_lane*72u+hidden]=tanh(value);
        hidden+=8u;}}
        workgroupBarrier();
        {var label=lane;loop{if(!(classify && label<9u)){break;}
            var value=i[35856u+label];
            {var hidden=0u;loop{if(!(hidden<72u)){break;}value+=i[25920u+label*72u+hidden]*Xb[token_lane*72u+hidden];hidden++;}}
            jb[token_lane*9u+label]=value;
        label+=8u;}}
        workgroupBarrier();
        if(lane==0u && valid) {
            var label=0u;
            if(classify) {
                var best=jb[token_lane*9u];
                {var candidate=1u;loop{if(!(candidate<9u)){break;}
                    let score=jb[token_lane*9u+candidate];
                    if(score>best) {best=score;label=candidate;}
                candidate++;}}
            }
            let ignored=atomicOr(&Fd[token/4u],label<<((token&3u)*8u));
        }
        workgroupBarrier();
    round++;}}
}

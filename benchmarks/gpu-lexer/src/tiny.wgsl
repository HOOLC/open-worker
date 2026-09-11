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
        for(var pos=k;pos>0u;pos--) {
            let kind=M[(pos-1u)*2u]&3u;
            if(kind!=1u && kind!=2u) {previous=pos-1u;break;}
        }
        for(var pos=k+1u;pos<count;pos++) {
            let kind=M[pos*2u]&3u;
            if(kind!=1u && kind!=2u) {next=pos;break;}
        }
        j[k*32u].y=bitcast<f32>(previous);
        j[k*32u].z=bitcast<f32>(next);
    }
    workgroupBarrier();
    for(var token=0u;token<count;token++) {
        j[token*32u+k].x=Ed(token,k,0u,count,j[token*32u].yz);
    }
    workgroupBarrier();
    for(var token=0u;token<count;token++) {
        let gate=Vb(token,k);
        j[token*32u+k].y=gate.x;
        j[token*32u+k].z=gate.y;
    }
    // For a single chunk the scan prefix/suffix state is exactly zero.
    var state=0.;
    for(var token=0u;token<count;token++) {
        let at=token*32u+k;
        state=j[at].y*state+j[at].z;
        j[at].w=state;
    }
    state=0.;
    for(var token=count;token>0u;token--) {
        let at=(token-1u)*32u+k;
        state=j[at].y*state+j[at].z;
        j[at].z=state;
    }
    workgroupBarrier();
    for(var token=0u;token<count;token++) {
        var mixed=j[token*32u+k].x+i[41289u+k];
        let weight=39241u+k*64u;
        for(var channel=0u;channel<32u;channel++) {
            let context=j[token*32u+channel];
            mixed+=context.w*i[weight+channel]+context.z*i[weight+32u+channel];
        }
        let value=f32(f16(tanh(mixed)));
        tiny_base[token*32u+k]=value;
        m[(31u+token)*32u+k]=value;
    }
    workgroupBarrier();
    var capacity=32u;
    var valid_nodes=count;
    var level=0u;
    loop {
        if(capacity<=1u) {break;}
        let parents=capacity/2u;
        let parent_base=parents-1u;
        let child_base=capacity-1u;
        for(var node=0u;node<parents;node++) {
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
        }
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
    for(var round=0u;round<8u;round++) {
        let token=round*4u+token_lane;
        let valid=token<count;
        var kind=1u;
        if(valid) {kind=M[token*2u]&3u;}
        let classify=valid && kind!=1u && kind!=2u;
        for(var gate=lane;classify && gate<16u;gate+=8u) {
            var value=i[36889u+gate];
            for(var channel=0u;channel<32u;channel++) {
                let base=tiny_base[token*32u+channel];
                let context=f32(f16(m[(31u+token)*32u+channel]));
                let weight=35865u+gate*64u;
                value+=i[weight+channel]*base+i[weight+32u+channel]*context;
            }
            Wb[token_lane*16u+gate]=fb(value);
        }
        workgroupBarrier();
        for(var hidden=lane;classify && hidden<72u;hidden+=8u) {
            var value=i[35400u+hidden];
            for(var channel=0u;channel<32u;channel++) {
                let base=tiny_base[token*32u+channel];
                let context=f32(f16(m[(31u+token)*32u+channel]));
                let weight=29640u+hidden*80u;
                value+=i[weight+channel]*base+i[weight+32u+channel]*context;
            }
            let extra=29640u+hidden*80u+64u;
            for(var gate=0u;gate<16u;gate++) {value+=i[extra+gate]*Wb[token_lane*16u+gate];}
            Xb[token_lane*72u+hidden]=tanh(value);
        }
        workgroupBarrier();
        for(var label=lane;classify && label<9u;label+=8u) {
            var value=i[35856u+label];
            for(var hidden=0u;hidden<72u;hidden++) {value+=i[25920u+label*72u+hidden]*Xb[token_lane*72u+hidden];}
            jb[token_lane*9u+label]=value;
        }
        workgroupBarrier();
        if(lane==0u && valid) {
            var label=0u;
            if(classify) {
                var best=jb[token_lane*9u];
                for(var candidate=1u;candidate<9u;candidate++) {
                    let score=jb[token_lane*9u+candidate];
                    if(score>best) {best=score;label=candidate;}
                }
            }
            let ignored=atomicOr(&Fd[token/4u],label<<((token&3u)*8u));
        }
        workgroupBarrier();
    }
}

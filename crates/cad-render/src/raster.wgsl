struct Triangle { a:vec4<f32>, b:vec4<f32>, c:vec4<f32>, color:vec4<f32> }
struct Segment { a:vec4<f32>, b:vec4<f32> }
struct Pixel { color:u32, face:u32, edge:u32, z:f32, first:u32, second:u32, third:u32, more:u32, slope_x:f32, slope_y:f32 }
struct Params { size:vec4<u32>, depth:vec4<f32> }
@group(0) @binding(0) var<storage,read> triangles:array<Triangle>;
@group(0) @binding(1) var<storage,read> segments:array<Segment>;
@group(0) @binding(2) var<storage,read_write> pixels:array<Pixel>;
@group(0) @binding(3) var<uniform> params:Params;
@compute @workgroup_size(8,8)
fn surface(@builtin(global_invocation_id) gid:vec3<u32>) {
 if gid.x>=params.size.x || gid.y>=params.size.y {return;}
 let at=gid.y*params.size.x+gid.x;let xy=vec2<f32>(gid.xy)+vec2(0.5);
 var z=-1e30;var face=0u;var slope=vec2(0.);var color=0x00f8f5f2u;
 for(var i=0u;i<params.size.z;i++) {
  let t=triangles[i]; let p=t.a.xyz;let q=t.b.xyz;let r=t.c.xyz;
  let det=(q.y-r.y)*(p.x-r.x)+(r.x-q.x)*(p.y-r.y);
  if abs(det)<1e-7 {continue;}
  let u=((q.y-r.y)*(xy.x-r.x)+(r.x-q.x)*(xy.y-r.y))/det;
  let v=((r.y-p.y)*(xy.x-r.x)+(p.x-r.x)*(xy.y-r.y))/det;let w=1.-u-v;
  if min(u,min(v,w))< -1e-7 {continue;}
  let here=u*p.z+v*q.z+w*r.z;
  if here>z {slope=vec2(((q.y-r.y)*(p.z-r.z)+(r.y-p.y)*(q.z-r.z))/det,((r.x-q.x)*(p.z-r.z)+(p.x-r.x)*(q.z-r.z))/det);z=here;face=u32(t.a.w);color=u32(t.color.x)|(u32(t.color.y)<<8u)|(u32(t.color.z)<<16u);}
 }
 pixels[at]=Pixel(color,face,0u,z,0u,0u,0u,0u,slope.x,slope.y);
}
@compute @workgroup_size(8,8)
fn edge(@builtin(global_invocation_id) gid:vec3<u32>) {
 if gid.x>=params.size.x || gid.y>=params.size.y {return;}
 let at=gid.y*params.size.x+gid.x;let xy=vec2<f32>(gid.xy);
 var first=0u;var second=0u;var third=0u;var more=0u;
 for(var i=0u;i<params.size.w;i++) {
  let s=segments[i];let a=s.a.xyz;let b=s.b.xyz;
  // Match CPU's half-pixel samples + square 2 px tolerance, preserving occlusion.
  let steps=max(1u,u32(ceil(length(b.xy-a.xy)*2.)));
  let delta=b.xy-a.xy;let denom=max(dot(delta,delta),1e-12);
  let nearest=clamp(dot(xy-a.xy,delta)/denom,0.,1.);
  let middle=i32(round(nearest*f32(steps)));var hit=false;
  for(var k=max(0,middle-8);k<=min(i32(steps),middle+8);k++) {
   let t=f32(k)/f32(steps);let p=mix(a,b,t);
   if all(abs(round(p.xy)-xy)<=vec2(2.)) && abs((p.z-(pixels[at].z+pixels[at].slope_x*(p.x-xy.x-0.5)+pixels[at].slope_y*(p.y-xy.y-0.5)))*params.depth.x)<0.05 {hit=true;break;}
  }
  if hit {
   let id=u32(s.a.w);
   if first==0u {first=id;} else if id!=first && id!=second && id!=third {
    if second==0u {second=id;} else if third==0u {third=id;} else {more=1u;}
   }
  }
 }
 pixels[at].first=first;pixels[at].second=second;pixels[at].third=third;pixels[at].more=more;
 pixels[at].edge=select(first,0xffffffffu,second!=0u);
}

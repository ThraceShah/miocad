struct Camera { x: vec4<f32>, y: vec4<f32>, z: vec4<f32>, viewport:vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
struct Output { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> }
@vertex fn vs(@location(0) point:vec3<f32>, @location(1) color:vec3<f32>) -> Output {
    let p=vec4<f32>(point,1.0);
    var out:Output;
    out.position=vec4<f32>(dot(camera.x,p),dot(camera.y,p),dot(camera.z,p),1.0);
    out.color=color;
    return out;
}
@vertex fn vs_edge(@location(0) point:vec3<f32>, @location(1) color:vec3<f32>, @location(2) other:vec3<f32>, @location(3) corner:vec2<f32>) -> Output {
    let p=vec4<f32>(point,1.0);let q=vec4<f32>(other,1.0);
    let a=vec3<f32>(dot(camera.x,p),dot(camera.y,p),dot(camera.z,p));
    let b=vec3<f32>(dot(camera.x,q),dot(camera.y,q),dot(camera.z,q));
    let delta=(b.xy-a.xy)*camera.viewport.xy;
    let normal=vec2<f32>(-delta.y,delta.x)/max(length(delta),0.00001);
    let position=mix(a,b,corner.x);
    var out:Output;
    out.position=vec4<f32>(position.xy+normal*corner.y*2.0/camera.viewport.xy,position.z-0.0001,1.0);
    out.color=color;
    return out;
}
@fragment fn fs(in:Output) -> @location(0) vec4<f32> {return vec4<f32>(in.color,1.0);}
@fragment fn fs_srgb(in:Output) -> @location(0) vec4<f32> {
    let c=in.color;
    let linear=select(c/12.92,pow((c+0.055)/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));
    return vec4<f32>(linear,1.0);
}

#version 450
layout(set=0,binding=0) uniform sampler2D surface_texture;
layout(set=0,binding=1) uniform sampler2D shadow_texture;
layout(location=0) in vec3 normal;
layout(location=1) in vec2 uv;
layout(location=2) in vec3 world;
layout(location=3) in vec3 tint;
layout(location=4) in vec3 view_direction;
layout(location=5) flat in int material;
layout(location=6) in vec3 shadow_position;
layout(location=0) out vec4 color;
const float PI=3.14159265;
const vec3 SUN=vec3(-0.49,0.80,0.35);
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
vec3 sky_reflection(vec3 d){
    float horizon=pow(1.0-max(d.y,0.0),3.0);
    vec3 sky=mix(vec3(0.19,0.37,0.58),vec3(0.87,0.75,0.57),horizon);
    sky+=vec3(1.0,0.75,0.42)*pow(max(dot(d,SUN),0.0),256.0)*2.0;
    return mix(vec3(0.2,0.22,0.21),sky,smoothstep(-0.03,0.1,d.y));
}
vec3 fresnel(float x,vec3 f0){return f0+(1.0-f0)*pow(1.0-x,5.0);}
float shadow(float n_dot_l){
    vec2 st=shadow_position.xy*0.5+0.5;
    if(any(lessThan(st,vec2(0)))||any(greaterThan(st,vec2(1)))||shadow_position.z>1.0)return 1.0;
    float bias=max(0.00015,0.00065*(1.0-n_dot_l));
    ivec2 size=textureSize(shadow_texture,0);
    vec2 texel_position=st*vec2(size)-0.5;
    ivec2 base=ivec2(floor(texel_position));
    vec2 fraction=fract(texel_position);
    float visibility=0;
    // Fractional tent weights avoid quantized shadow steps when the camera moves.
    for(int y=-2;y<=3;y++)for(int x=-2;x<=3;x++){
        float depth=texelFetch(shadow_texture,clamp(base+ivec2(x,y),ivec2(0),size-ivec2(1)),0).r;
        float weight=max(0.0,3.0-abs(float(x)-fraction.x))*max(0.0,3.0-abs(float(y)-fraction.y));
        visibility+=(shadow_position.z-bias<=depth?1.0:0.0)*weight;
    }
    return visibility/81.0;
}
vec3 tonemap(vec3 c){return clamp((c*(2.51*c+0.03))/(c*(2.43*c+0.59)+0.14),0.0,1.0);}
void main(){
    vec3 n=normalize(normal),v=normalize(view_direction),l=SUN,h=normalize(v+l);
    vec2 coords=abs(n.y)>0.5?world.xz:(abs(n.x)>0.5?world.zy:world.xy);
    float grain=texture(surface_texture,coords*0.36).r;
    vec3 base;float roughness=0.65,metal=0;vec3 emission=vec3(0);
    if(material==0){
        vec2 tile=coords*0.43;vec2 edge=abs(fract(tile)-0.5);
        float edge_width=max(fwidth(tile.x),fwidth(tile.y));
        float grout=smoothstep(0.49-edge_width,0.499+edge_width,max(edge.x,edge.y));
        grout=mix(grout,0.015,smoothstep(0.2,0.8,edge_width));
        base=mix(vec3(0.24,0.27,0.27)*(0.85+0.24*hash(floor(tile))),vec3(0.09),grout);
        base*=0.9+0.14*grain;roughness=0.48;
        float ring=abs(length(world.xz)-6.8);
        float ring_aa=max(fwidth(ring),0.002);
        float ring_coverage=1.0-smoothstep(0.02-ring_aa,0.03+ring_aa,ring);
        base=mix(base,vec3(0.67,0.48,0.23),ring_coverage);metal=mix(metal,0.8,ring_coverage);roughness=mix(roughness,0.28,ring_coverage);
    }else if(material==1){
        vec2 facade=coords*vec2(1.4,1.0);
        vec2 cell=fract(facade);
        vec2 footprint=fwidth(facade);
        vec2 distance_to_line=min(cell,1.0-cell);
        vec2 lines=vec2(1.0)-smoothstep(vec2(0.035)-footprint,vec2(0.055)+footprint,distance_to_line);
        float frame=max(lines.x,lines.y);
        frame=mix(frame,0.16,smoothstep(0.25,0.8,max(footprint.x,footprint.y)));
        base=mix(vec3(0.055,0.14,0.19)*tint,vec3(0.24,0.29,0.29),frame);
        roughness=mix(0.14,0.36,frame);metal=mix(0.65,0.78,frame);
        float window_light=step(0.76,hash(floor(facade)+floor(world.xz*0.1)))*(1.0-frame);
        emission=vec3(1.0,0.64,0.27)*window_light*0.16;
    }else if(material==2){base=vec3(0.67,0.64,0.56)*tint*(0.92+grain*0.1);roughness=0.74;}
    else if(material==3){base=tint*vec3(0.83,0.78,0.64);roughness=0.24+grain*0.05;metal=0.95;}
    else if(material==4){base=vec3(0.62,0.58,0.48)*(0.9+grain*0.12);roughness=0.32;}
    else{base=vec3(0.1,0.13,0.14);metal=0.75;roughness=0.3;if(n.y>0.5)emission=vec3(1.6,1.0,0.4);}
    vec3 dx=dFdx(n),dy=dFdy(n);
    roughness=sqrt(clamp(roughness*roughness+0.35*(dot(dx,dx)+dot(dy,dy)),0.02,1.0));
    float nl=max(dot(n,l),0.0),nv=max(dot(n,v),0.001),nh=max(dot(n,h),0.0),vh=max(dot(v,h),0.0);
    vec3 f0=mix(vec3(0.04),base,metal),f=fresnel(vh,f0);
    float a=roughness*roughness,a2=a*a;
    float denominator=nh*nh*(a2-1.0)+1.0;
    float distribution=a2/max(PI*denominator*denominator,0.0001);
    float k=(roughness+1.0)*(roughness+1.0)/8.0;
    float geometry=(nv/(nv*(1.0-k)+k))*(nl/(nl*(1.0-k)+k));
    vec3 specular=distribution*geometry*f/max(4.0*nv*nl,0.001);
    vec3 diffuse=(1.0-f)*(1.0-metal)*base/PI;
    float visibility=shadow(nl);
    vec3 sunlight=vec3(3.4,2.85,2.1)*(diffuse+specular)*nl*visibility;
    vec3 ambient=base*(1.0-metal)*mix(vec3(0.13,0.16,0.19),vec3(0.25,0.31,0.39),n.y*0.5+0.5);
    vec3 reflected=sky_reflection(reflect(-v,n));
    vec3 environment=reflected*fresnel(nv,f0)*(1.0-roughness*0.68);
    float contact=material==0?mix(0.68,1.0,smoothstep(2.5,4.0,length(world.xz))):mix(0.7,1.0,smoothstep(0.2,1.5,world.y));
    vec3 lit=sunlight+(ambient+environment)*contact+emission;
    float fog=1.0-exp(-length(view_direction)*0.012);
    lit=mix(lit,vec3(0.60,0.64,0.64),fog*0.7);
    color=vec4(tonemap(lit),1.0);
}

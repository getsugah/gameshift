#version 450
layout(location=0) in vec3 direction;
layout(location=0) out vec4 color;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
float fbm(vec2 p){float r=0,a=0.5;for(int i=0;i<5;i++){r+=noise(p)*a;p=mat2(1.6,1.2,-1.2,1.6)*p+vec2(7.1,1.3);a*=0.5;}return r;}
vec3 tonemap(vec3 c){return clamp((c*(2.51*c+0.03))/(c*(2.43*c+0.59)+0.14),0.0,1.0);}
void main(){
    vec3 d=normalize(direction),sun=normalize(vec3(-0.49,0.80,0.35));
    float horizon=pow(1.0-max(d.y,0.0),6.0);
    vec3 sky=mix(vec3(0.045,0.18,0.38),vec3(0.60,0.73,0.82),horizon);
    float alignment=max(dot(d,sun),0.0);
    sky+=vec3(1.0,0.78,0.42)*pow(alignment,32.0)*0.45;
    sky+=vec3(7.0,5.5,3.2)*smoothstep(0.9994,0.99975,alignment);
    vec2 plane=d.xz/max(d.y,0.07)*1.2;
    float clouds=smoothstep(0.46,0.74,fbm(plane+vec2(3.1,9.7)));
    clouds*=smoothstep(0.02,0.25,d.y);
    vec3 cloud=mix(vec3(0.73,0.76,0.78),vec3(1.4,1.25,1.0),alignment);
    sky=mix(sky,cloud,clouds*0.78);
    color=vec4(tonemap(sky),1);
}

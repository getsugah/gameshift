#version 450
layout(push_constant) uniform Scene { mat4 view_projection; vec4 parameters; vec4 light_rows[3]; } scene;
layout(location=0) out vec3 direction;
void main(){
    vec2 pos=vec2((gl_VertexIndex<<1)&2,gl_VertexIndex&2)*2.0-1.0;
    float t=scene.parameters.y,a=t*0.075+0.65;
    vec3 eye=vec3(cos(a)*9.5,6.3+sin(t*0.11)*0.35,sin(a)*9.5);
    vec4 point=inverse(scene.view_projection)*vec4(pos,1,1);
    direction=point.xyz/point.w-eye;
    gl_Position=vec4(pos,1,1);
}

#version 450
layout(push_constant) uniform Scene { mat4 view_projection; vec4 parameters; vec4 light_rows[3]; } scene;
layout(location=0) out vec3 normal;
layout(location=1) out vec2 uv;
layout(location=2) out vec3 world;
layout(location=3) out vec3 tint;
layout(location=4) out vec3 view_direction;
layout(location=5) flat out int material;
layout(location=6) out vec3 shadow_position;
const vec3 normals[6]=vec3[](vec3(1,0,0),vec3(-1,0,0),vec3(0,1,0),vec3(0,-1,0),vec3(0,0,1),vec3(0,0,-1));
const vec3 tangents[6]=vec3[](vec3(0,0,-1),vec3(0,0,1),vec3(1,0,0),vec3(1,0,0),vec3(1,0,0),vec3(-1,0,0));
const vec3 bitangents[6]=vec3[](vec3(0,1,0),vec3(0,1,0),vec3(0,0,-1),vec3(0,0,1),vec3(0,1,0),vec3(0,1,0));
const vec2 corners[6]=vec2[](vec2(-1,-1),vec2(1,-1),vec2(1,1),vec2(-1,-1),vec2(1,1),vec2(-1,1));
float edge_coordinate(int index,float length,float radius){return index==0?-length:(index==1?-length+radius:(index==2?length-radius:length));}
float hash(float x){return fract(sin(x*127.1+311.7)*43758.5453);}
mat3 rotate_y(float a){float c=cos(a),s=sin(a);return mat3(c,0,-s,0,1,0,s,0,c);}
mat3 rotate_x(float a){float c=cos(a),s=sin(a);return mat3(1,0,0,0,c,s,0,-s,c);}
void main(){
    int instance=gl_InstanceIndex;
    int side=int(scene.parameters.x);
    float time=scene.parameters.y;
    float angle=time*0.075+0.65;
    vec3 eye=vec3(cos(angle)*9.5,6.3+sin(time*0.11)*0.35,sin(angle)*9.5);
    material=0; tint=vec3(1);
    if(instance>=1000000 && instance<1000003){
        // Three real tessellated toroidal meshes, no ray marching or impostor.
        int quad=gl_VertexIndex/6;
        vec2 corner=corners[gl_VertexIndex%6]*0.5+0.5;corner.y=1.0-corner.y;
        float u=(float(quad/16)+corner.x)*6.28318530718/64.0;
        float v=(float(quad%16)+corner.y)*6.28318530718/16.0;
        vec3 local=vec3((2.0+0.19*cos(v))*cos(u),0.19*sin(v),(2.0+0.19*cos(v))*sin(u));
        vec3 n=vec3(cos(v)*cos(u),sin(v),cos(v)*sin(u));
        mat3 rotation=rotate_y(float(instance-1000000)*1.04719755)*rotate_x(1.05);
        world=rotation*local+vec3(0,3.0,0);
        normal=rotation*n;uv=vec2(u,v)*5.0;material=3;
        tint=vec3(0.83,0.59,0.27);
    } else {
        int face=gl_VertexIndex/54;
        int face_quad=(gl_VertexIndex%54)/6;
        vec2 corner=corners[gl_VertexIndex%6]*0.5+0.5;
        vec3 scale,center;float rotation_angle=0;
        if(instance==0){scale=vec3(float(side)*2.3,0.12,float(side)*2.3);center=vec3(0,-0.14,0);material=0;}
        else if(instance==1000010){scale=vec3(2.7,0.35,2.7);center=vec3(0,0.35,0);material=4;}
        else if(instance>=1000100){
            float a=float(instance-1000100)*6.28318530718/32.0;
            center=vec3(cos(a)*8.25,0.44,sin(a)*8.25);scale=vec3(0.09,0.44,0.09);material=5;
        }else{
            int id=(instance-1)/8;int part=(instance-1)%8;
            int x=id%side,z=id/side;
            vec2 position=(vec2(x,z)-float(side-1)*0.5)*3.5;
            if(length(position)<11.5){gl_Position=vec4(2,2,2,1);return;}
            float h=2.0+4.5*hash(float(id)+3.0);
            center=vec3(position.x,h+0.6,position.y);scale=vec3(0.88,h,0.88);material=1;
            rotation_angle=float((x+z)%4)*1.57079632679;
            vec3 offset=vec3(0);
            if(part==1){scale=vec3(1.33,0.3,1.33);center.y=0.3;material=2;}
            if(part==2){scale=vec3(0.99,0.13,0.99);center.y=2.0*h+0.72;material=3;}
            if(part==3){scale=vec3(0.46,h*0.62,0.67);center.y=h*0.62+0.6;offset.x=1.12;material=2;}
            if(part==4){scale=vec3(0.54,0.1,0.74);center.y=1.24*h+0.72;offset.x=1.12;material=3;}
            if(part==5){scale=vec3(0.09,h,0.94);offset.x=-0.84;material=3;}
            if(part==6){scale=vec3(0.32,0.28,0.3);center.y=2.0*h+1.12;offset=vec3(0.22,0,0.18);material=2;}
            if(part==7){scale=vec3(0.035,0.85,0.035);center.y=2.0*h+1.62;material=3;}
            center+=rotate_y(rotation_angle)*offset;
            tint=mix(vec3(0.62,0.71,0.73),vec3(0.92,0.83,0.67),hash(float(id)+19.0));
        }
        mat3 rotation=rotate_y(rotation_angle);
        float radius=min(0.065,min(scale.x,min(scale.y,scale.z))*0.16);
        float u_length=dot(abs(tangents[face]),scale),v_length=dot(abs(bitangents[face]),scale);
        float u=edge_coordinate(face_quad/3+int(corner.x),u_length,radius);
        float v=edge_coordinate(face_quad%3+int(corner.y),v_length,radius);
        vec3 point=normals[face]*scale+tangents[face]*u+bitangents[face]*v;
        vec3 core=clamp(point,-scale+radius,scale-radius);
        vec3 local_normal=normalize(point-core);
        vec3 local=core+local_normal*radius;
        world=center+rotation*local;normal=rotation*local_normal;
        uv=vec2(u,v);
    }
    view_direction=eye-world;
    vec4 w=vec4(world,1);
    shadow_position=vec3(dot(scene.light_rows[0],w),dot(scene.light_rows[1],w),dot(scene.light_rows[2],w));
    gl_Position=scene.parameters.w>0.5?vec4(shadow_position,1):scene.view_projection*w;
}

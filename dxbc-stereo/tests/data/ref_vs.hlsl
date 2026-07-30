cbuffer StereoConstants : register(b13) { row_major float4x4 ViewProj[2]; float4 CameraPos[2]; };
struct VSIn  { float3 pos : POSITION; uint iid : SV_InstanceID; };
struct VSOut { float4 clip : SV_Position; uint vp : SV_ViewportArrayIndex; };
VSOut main(VSIn i){ uint eye=i.iid&1; float3 rel=i.pos-CameraPos[eye].xyz; VSOut o; o.clip=mul(float4(rel,1.0),ViewProj[eye]); o.vp=eye; return o; }

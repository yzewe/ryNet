#version 440
// The quad under state.frag, as a ShaderEffect draws it.
layout(location = 0) in vec4 qt_Vertex;
layout(location = 1) in vec2 qt_MultiTexCoord0;
layout(location = 0) out vec2 qt_TexCoord0;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 tint;
    vec4 ringColor;
    vec4 radii;
    vec2 size;
    vec2 center;
    float ripple;
    float rippleAlpha;
    float stateAlpha;
    float inset;
    float ring;
};

void main()
{
    qt_TexCoord0 = qt_MultiTexCoord0;
    gl_Position = qt_Matrix * qt_Vertex;
}

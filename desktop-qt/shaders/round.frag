#version 440
// An image clipped to a rounded rectangle (a circle when the radius is half the size).
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec2 size;
    float radius;
};
layout(binding = 1) uniform sampler2D source;

void main()
{
    vec2 c = qt_TexCoord0 * size - size * 0.5;
    float r = min(radius, min(size.x, size.y) * 0.5);
    vec2 q = abs(c) - size * 0.5 + r;
    float d = min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
    fragColor = texture(source, qt_TexCoord0) * clamp(0.5 - d, 0.0, 1.0) * qt_Opacity;
}

#version 440
// A Material state layer: a hover/press tint plus a ripple circle, clipped to a rounded rectangle, and the
// focus ring around it. The shape sits `inset` in from the item's edges, leaving room for the ring.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 tint;
    vec4 ringColor;
    vec4 radii; // top left, top right, bottom right, bottom left
    vec2 size;
    vec2 center;
    float ripple;
    float rippleAlpha;
    float stateAlpha;
    float inset;
    float ring;
};

float corner(vec2 p)
{
    if (p.x < 0.0)
        return p.y < 0.0 ? radii.x : radii.w;
    return p.y < 0.0 ? radii.y : radii.z;
}

void main()
{
    vec2 p = qt_TexCoord0 * size;
    vec2 c = p - size * 0.5;
    vec2 half_ = size * 0.5 - inset;
    float r = min(corner(c), min(half_.x, half_.y));
    vec2 q = abs(c) - half_ + r;
    float d = min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
    float inside = clamp(0.5 - d, 0.0, 1.0);
    // A soft edge that widens with the ripple: a glow spreading from the press, not a ring sweeping across.
    float soft = max(6.0, ripple * 0.35);
    float rd = length(p - center) - ripple;
    float wave = clamp(0.5 - rd / soft, 0.0, 1.0);
    float a = clamp(stateAlpha + rippleAlpha * wave, 0.0, 1.0) * inside * tint.a;
    // The focus ring: 3 px wide, 2 px out from the shape at rest; it springs out from under the edge.
    float outer = 2.0 + 3.0 * ring;
    float band = clamp(d - outer + 3.5, 0.0, 1.0) * clamp(outer + 0.5 - d, 0.0, 1.0) * clamp(ring, 0.0, 1.0) * ringColor.a;
    fragColor = (vec4(ringColor.rgb, 1.0) * band + vec4(tint.rgb, 1.0) * a * (1.0 - band)) * qt_Opacity;
}

#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>

namespace Hct {

namespace detail {
inline constexpr double kPi = 3.14159265358979323846;

inline double yFromLstar(double l)
{
    const double ft = (l + 16.0) / 116.0, ft3 = ft * ft * ft;
    return 100.0 * (ft3 > 216.0 / 24389.0 ? ft3 : (116.0 * ft - 16.0) / (24389.0 / 27.0));
}

inline double linearized(int c)
{
    const double n = c / 255.0;
    return (n <= 0.040449936 ? n / 12.92 : std::pow((n + 0.055) / 1.055, 2.4)) * 100.0;
}

inline int delinearized(double c)
{
    const double n = c / 100.0;
    const double d = n <= 0.0031308 ? n * 12.92 : 1.055 * std::pow(n, 1.0 / 2.4) - 0.055;
    return std::clamp(int(std::lround(d * 255.0)), 0, 255);
}

inline uint32_t argb(int r, int g, int b) { return 0xff000000u | uint32_t(r) << 16 | uint32_t(g) << 8 | uint32_t(b); }

struct Vc {
    double n, aw, nbb, ncb, c, nc, fl, fLRoot, z;
    double rgbD[3];
};

inline const Vc &vc()
{
    static const Vc v = [] {
        const double w[3] = {95.047, 100.0, 108.883};
        const double adapting = 200.0 / kPi * yFromLstar(50.0) / 100.0;
        const double rW = w[0] * 0.401288 + w[1] * 0.650173 + w[2] * -0.051461;
        const double gW = w[0] * -0.250268 + w[1] * 1.204414 + w[2] * 0.045854;
        const double bW = w[0] * -0.002079 + w[1] * 0.048952 + w[2] * 0.953127;
        const double f = 0.8 + 2.0 / 10.0;
        const double c = f >= 0.9 ? 0.59 + (0.69 - 0.59) * ((f - 0.9) * 10.0) : 0.525 + (0.59 - 0.525) * ((f - 0.8) * 10.0);
        const double d = std::clamp(f * (1.0 - (1.0 / 3.6) * std::exp((-adapting - 42.0) / 92.0)), 0.0, 1.0);
        Vc o{};
        o.nc = f;
        o.c = c;
        o.rgbD[0] = d * (100.0 / rW) + 1.0 - d;
        o.rgbD[1] = d * (100.0 / gW) + 1.0 - d;
        o.rgbD[2] = d * (100.0 / bW) + 1.0 - d;
        const double k = 1.0 / (5.0 * adapting + 1.0), k4 = k * k * k * k, k4F = 1.0 - k4;
        o.fl = k4 * adapting + 0.1 * k4F * k4F * std::cbrt(5.0 * adapting);
        o.fLRoot = std::pow(o.fl, 0.25);
        o.n = yFromLstar(50.0) / w[1];
        o.z = 1.48 + std::sqrt(o.n);
        o.nbb = 0.725 / std::pow(o.n, 0.2);
        o.ncb = o.nbb;
        double a[3];
        const double white[3] = {rW, gW, bW};
        for (int i = 0; i < 3; ++i) {
            const double af = std::pow(o.fl * o.rgbD[i] * white[i] / 100.0, 0.42);
            a[i] = 400.0 * af / (af + 27.13);
        }
        o.aw = (2.0 * a[0] + a[1] + 0.05 * a[2]) * o.nbb;
        return o;
    }();
    return v;
}

inline std::array<double, 3> linrgb(double j, double chroma, double hue)
{
    const Vc &v = vc();
    const double alpha = chroma == 0.0 || j == 0.0 ? 0.0 : chroma / std::sqrt(j / 100.0);
    const double t = std::pow(alpha / std::pow(1.64 - std::pow(0.29, v.n), 0.73), 1.0 / 0.9);
    const double h = hue * kPi / 180.0;
    const double eHue = 0.25 * (std::cos(h + 2.0) + 3.8);
    const double ac = v.aw * std::pow(j / 100.0, 1.0 / v.c / v.z);
    const double p1 = eHue * (50000.0 / 13.0) * v.nc * v.ncb;
    const double p2 = ac / v.nbb;
    const double hs = std::sin(h), hc = std::cos(h);
    const double gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * hc + 108.0 * t * hs);
    const double a = gamma * hc, b = gamma * hs;
    const double rgbA[3] = {(460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0, (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0,
                            (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0};
    double rgbF[3];
    for (int i = 0; i < 3; ++i) {
        const double base = std::max(0.0, 27.13 * std::abs(rgbA[i]) / (400.0 - std::abs(rgbA[i])));
        const double sign = rgbA[i] < 0 ? -1.0 : rgbA[i] > 0 ? 1.0 : 0.0;
        rgbF[i] = sign * (100.0 / v.fl) * std::pow(base, 1.0 / 0.42) / v.rgbD[i];
    }
    const double x = 1.86206786 * rgbF[0] - 1.01125463 * rgbF[1] + 0.14918677 * rgbF[2];
    const double y = 0.38752654 * rgbF[0] + 0.62144744 * rgbF[1] - 0.00897398 * rgbF[2];
    const double z = -0.01584150 * rgbF[0] - 0.03412294 * rgbF[1] + 1.04996444 * rgbF[2];
    return {3.2413774792388685 * x - 1.5376652402851851 * y - 0.49885366846268053 * z,
            -0.9691452513005321 * x + 1.8758853451067872 * y + 0.04156585616912061 * z,
            0.05562093689691305 * x - 0.20395524564742123 * y + 1.0571799111220335 * z};
}

inline double luminance(const std::array<double, 3> &c) { return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]; }
}

inline double hue(uint32_t argb)
{
    using namespace detail;
    const Vc &v = vc();
    const double r = linearized(int(argb >> 16 & 255)), g = linearized(int(argb >> 8 & 255)), b = linearized(int(argb & 255));
    const double x = 0.41233895 * r + 0.35762064 * g + 0.18051042 * b;
    const double y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const double z = 0.01932141 * r + 0.11916382 * g + 0.95034478 * b;
    const double c[3] = {0.401288 * x + 0.650173 * y - 0.051461 * z, -0.250268 * x + 1.204414 * y + 0.045854 * z,
                         -0.002079 * x + 0.048952 * y + 0.953127 * z};
    double a3[3];
    for (int i = 0; i < 3; ++i) {
        const double d = v.rgbD[i] * c[i];
        const double af = std::pow(v.fl * std::abs(d) / 100.0, 0.42);
        a3[i] = (d < 0 ? -1.0 : d > 0 ? 1.0 : 0.0) * 400.0 * af / (af + 27.13);
    }
    const double a = (11.0 * a3[0] - 12.0 * a3[1] + a3[2]) / 11.0;
    const double bb = (a3[0] + a3[1] - 2.0 * a3[2]) / 9.0;
    const double deg = std::atan2(bb, a) * 180.0 / kPi;
    return deg < 0 ? deg + 360.0 : deg >= 360 ? deg - 360.0 : deg;
}

inline uint32_t from(double hue, double chroma, double tone)
{
    using namespace detail;
    hue = std::fmod(std::fmod(hue, 360.0) + 360.0, 360.0);
    if (chroma < 1e-4 || tone < 1e-4 || tone > 99.9999) {
        const int g = delinearized(yFromLstar(std::clamp(tone, 0.0, 100.0)));
        return argb(g, g, g);
    }
    const double target = yFromLstar(tone);
    auto solve = [&](double c, std::array<double, 3> &out) {
        double lo = 0.0, hi = 100.0;
        for (int i = 0; i < 30; ++i) {
            const double mid = (lo + hi) / 2;
            const double y = luminance(linrgb(mid, c, hue));
            (std::isnan(y) || y < target ? lo : hi) = mid;
        }
        out = linrgb(hi, c, hue);
        constexpr double e = 0.01;
        if (std::abs(luminance(out) - target) > 0.05 * std::max(0.1, target))
            return false;
        return std::all_of(out.begin(), out.end(), [](double v) { return v >= -e && v <= 100.0 + e; });
    };
    std::array<double, 3> best{};
    if (!solve(chroma, best)) {
        double lo = 0.0, hi = chroma;
        bool found = false;
        for (int i = 0; i < 16; ++i) {
            const double mid = (lo + hi) / 2;
            std::array<double, 3> c{};
            if (solve(mid, c)) {
                best = c;
                found = true;
                lo = mid;
            } else {
                hi = mid;
            }
        }
        if (!found) {
            const int g = delinearized(target);
            return argb(g, g, g);
        }
    }
    return argb(delinearized(best[0]), delinearized(best[1]), delinearized(best[2]));
}

struct Palette {
    double hue, chroma;
    uint32_t tone(double t) const { return from(hue, chroma, t); }
};

inline std::array<Palette, 5> tonalSpot(uint32_t seed)
{
    const double h = hue(seed);
    return {Palette{h, 36.0}, Palette{h, 16.0}, Palette{std::fmod(h + 60.0, 360.0), 24.0}, Palette{h, 6.0}, Palette{h, 8.0}};
}

enum { Primary, Secondary, Tertiary, Neutral, NeutralVariant };

struct RoleTone {
    const char *name;
    int palette;
    double light, dark;
};
inline constexpr RoleTone kRoles[] = {
    {"primary", Primary, 40, 80},
    {"onPrimary", Primary, 100, 20},
    {"primaryContainer", Primary, 90, 30},
    {"onPrimaryContainer", Primary, 30, 90},
    {"inversePrimary", Primary, 80, 40},
    {"surfaceTint", Primary, 40, 80},
    {"secondary", Secondary, 40, 80},
    {"onSecondary", Secondary, 100, 20},
    {"secondaryContainer", Secondary, 90, 30},
    {"onSecondaryContainer", Secondary, 30, 90},
    {"tertiary", Tertiary, 40, 80},
    {"onTertiary", Tertiary, 100, 20},
    {"tertiaryContainer", Tertiary, 90, 30},
    {"onTertiaryContainer", Tertiary, 30, 90},
    {"background", Neutral, 98, 6},
    {"onBackground", Neutral, 10, 90},
    {"surface", Neutral, 98, 6},
    {"onSurface", Neutral, 10, 90},
    {"surfaceVariant", NeutralVariant, 90, 30},
    {"onSurfaceVariant", NeutralVariant, 30, 80},
    {"inverseSurface", Neutral, 20, 90},
    {"inverseOnSurface", Neutral, 95, 20},
    {"outline", NeutralVariant, 50, 60},
    {"outlineVariant", NeutralVariant, 80, 30},
    {"surfaceBright", Neutral, 98, 24},
    {"surfaceDim", Neutral, 87, 6},
    {"surfaceContainerLowest", Neutral, 100, 4},
    {"surfaceContainerLow", Neutral, 96, 10},
    {"surfaceContainer", Neutral, 94, 12},
    {"surfaceContainerHigh", Neutral, 92, 17},
    {"surfaceContainerHighest", Neutral, 90, 22},
    {"primaryFixed", Primary, 90, 90},
    {"primaryFixedDim", Primary, 80, 80},
    {"onPrimaryFixed", Primary, 10, 10},
    {"onPrimaryFixedVariant", Primary, 30, 30},
    {"secondaryFixed", Secondary, 90, 90},
    {"secondaryFixedDim", Secondary, 80, 80},
    {"onSecondaryFixed", Secondary, 10, 10},
    {"onSecondaryFixedVariant", Secondary, 30, 30},
    {"tertiaryFixed", Tertiary, 90, 90},
    {"tertiaryFixedDim", Tertiary, 80, 80},
    {"onTertiaryFixed", Tertiary, 10, 10},
    {"onTertiaryFixedVariant", Tertiary, 30, 30},
};

}

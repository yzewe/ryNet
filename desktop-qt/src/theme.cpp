#include "theme.h"

#include "hct.h"
#include "platform.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPointF>
#include <QRegularExpression>
#include <QSettings>
#include <QVariantList>
#include <cmath>
#include <cstring>

#ifdef RYNET_DBUS
#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDBusVariant>
#endif

namespace {

struct Lch {
    double l, c, h;
};

double lin(double v) { return v <= 0.04045 ? v / 12.92 : std::pow((v + 0.055) / 1.055, 2.4); }

Lch lch(const QColor &col)
{
    const double r = lin(col.redF()), g = lin(col.greenF()), b = lin(col.blueF());
    const double x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    const double y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const double z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    auto f = [](double t) { return t > 216.0 / 24389 ? std::cbrt(t) : (24389.0 / 27 * t + 16) / 116; };
    const double l = 116 * f(y) - 16;
    const double a = 500 * (f(x) - f(y));
    const double bb = 200 * (f(y) - f(z));
    return {l, std::hypot(a, bb), std::atan2(bb, a) * 180.0 / M_PI};
}

std::optional<QColor> lchToRgb(double l, double c, double hueDeg)
{
    const double h = hueDeg * M_PI / 180.0;
    const double a = c * std::cos(h), b = c * std::sin(h);
    const double fy = (l + 16) / 116, fx = fy + a / 500, fz = fy - b / 200;
    auto inv = [](double t) { return t * t * t > 216.0 / 24389 ? t * t * t : (116 * t - 16) / (24389.0 / 27); };
    const double x = inv(fx) * 0.95047;
    const double y = l > 8 ? fy * fy * fy : l / (24389.0 / 27);
    const double z = inv(fz) * 1.08883;
    const double r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    const double g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    const double bl = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    const double e = 1e-4;
    if (r < -e || g < -e || bl < -e || r > 1 + e || g > 1 + e || bl > 1 + e)
        return {};
    auto gam = [](double v) {
        const double u = std::clamp(v, 0.0, 1.0);
        return u <= 0.0031308 ? 12.92 * u : 1.055 * std::pow(u, 1 / 2.4) - 0.055;
    };
    return QColor::fromRgbF(float(gam(r)), float(gam(g)), float(gam(bl)));
}

QColor fromLch(double l, double chroma, double hue)
{
    for (double c = chroma;; c -= 1.0) {
        if (const auto rgb = lchToRgb(l, c, hue))
            return *rgb;
        if (c <= 0.0)
            return lchToRgb(l, 0.0, hue).value_or(QColor(Qt::gray));
    }
}

QColor harmonize(const QColor &c, const QColor &toward)
{
    const Lch a = lch(c);
    const double target = lch(toward).h;
    const double diff = std::fmod(target - a.h + 540, 360) - 180;
    const double rotation = std::min(std::abs(diff) * 0.5, 15.0) * (diff < 0 ? -1 : diff > 0 ? 1 : 0);
    return fromLch(a.l, a.c, a.h + rotation);
}

QColor rgb(quint32 v) { return QColor::fromRgb(QRgb(v)); }

QColor lerp(const QColor &a, const QColor &b, double t)
{
    return QColor::fromRgbF(float(a.redF() + (b.redF() - a.redF()) * t), float(a.greenF() + (b.greenF() - a.greenF()) * t),
                            float(a.blueF() + (b.blueF() - a.blueF()) * t), float(a.alphaF() + (b.alphaF() - a.alphaF()) * t));
}

using namespace Role;

void set(Scheme &s, std::initializer_list<std::pair<int, quint32>> values)
{
    for (const auto &[role, v] : values)
        s[role] = rgb(v);
}

QString home() { return QDir::homePath(); }

QString env(const char *name, const QString &fallback)
{
    const QString v = qEnvironmentVariable(name);
    return v.isEmpty() ? fallback : v;
}

std::optional<QColor> hex(const QString &s)
{
    QString h = s.trimmed();
    if (h.startsWith(u'#'))
        h = h.mid(1);
    if (h.size() != 6)
        return {};
    bool ok = false;
    const uint v = h.toUInt(&ok, 16);
    if (!ok)
        return {};
    return QColor::fromRgb(QRgb(0xff000000u | v));
}

QJsonObject readJson(const QString &file)
{
    QFile f(file);
    if (!f.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(f.readAll()).object();
}

#ifdef RYNET_DBUS
std::optional<QVariant> portalRead(const QString &key)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return {};
    for (const char *method : {"ReadOne", "Read"}) {
        QDBusMessage m = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.portal.Desktop"), QStringLiteral("/org/freedesktop/portal/desktop"),
                                                        QStringLiteral("org.freedesktop.portal.Settings"), QLatin1String(method));
        m << QStringLiteral("org.freedesktop.appearance") << key;
        const QDBusMessage r = bus.call(m, QDBus::Block, 1500);
        if (r.type() != QDBusMessage::ReplyMessage || r.arguments().isEmpty())
            continue;
        QVariant v = r.arguments().first();
        while (v.canConvert<QDBusVariant>() && v.userType() == qMetaTypeId<QDBusVariant>())
            v = v.value<QDBusVariant>().variant();
        return v;
    }
    return {};
}
#endif

std::optional<bool> dmsLight()
{
    const QJsonObject o = readJson(env("XDG_STATE_HOME", home() + QStringLiteral("/.local/state")) + QStringLiteral("/DankMaterialShell/session.json"));
    if (!o.contains("isLightMode"))
        return {};
    return o.value("isLightMode").toBool();
}

bool systemDark()
{
#ifdef Q_OS_WIN
    QSettings s(QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), QSettings::NativeFormat);
    return s.contains(QStringLiteral("AppsUseLightTheme")) && s.value(QStringLiteral("AppsUseLightTheme")).toInt() == 0;
#else
#ifdef RYNET_DBUS
    if (const auto v = portalRead(QStringLiteral("color-scheme"))) {
        const uint scheme = v->toUInt();
        if (scheme == 1)
            return true;
        if (scheme == 2)
            return false;
    }
#endif
    if (const auto g = Platform::out(QStringLiteral("gsettings"), {QStringLiteral("get"), QStringLiteral("org.gnome.desktop.interface"), QStringLiteral("color-scheme")})) {
        if (g->contains(QStringLiteral("prefer-dark")))
            return true;
        if (g->contains(QStringLiteral("prefer-light")))
            return false;
    }
    if (const auto light = dmsLight())
        return !*light;
    return false;
#endif
}

std::optional<QColor> systemSeed(bool dark)
{
#ifdef Q_OS_WIN
    QSettings s(QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM"), QSettings::NativeFormat);
    if (!s.contains(QStringLiteral("AccentColor")))
        return {};
    const quint32 v = s.value(QStringLiteral("AccentColor")).toUInt();
    return QColor(int(v & 0xff), int((v >> 8) & 0xff), int((v >> 16) & 0xff));
#else
    const QString config = env("XDG_CONFIG_HOME", home() + QStringLiteral("/.config"));
    const QString cache = env("XDG_CACHE_HOME", home() + QStringLiteral("/.cache"));
    const QJsonObject dms = readJson(config + QStringLiteral("/DankMaterialShell/settings.json"));
    if (!dms.isEmpty()) {
        const QString theme = dms.value("currentThemeName").toString();
        if (theme == u"custom") {
            const QString file = dms.value("customThemeFile").toString();
            if (!file.isEmpty())
                if (const auto c = hex(readJson(file).value(dark ? "dark" : "light").toObject().value("primary").toString()))
                    return c;
        } else if (theme == u"dynamic") {
            const QJsonObject colors = readJson(cache + QStringLiteral("/DankMaterialShell/dms-colors.json"));
            if (const auto c = hex(colors.value("colors").toObject().value("dark").toObject().value("source_color").toString()))
                return c;
        }
    }
    static const QRegularExpression accent(QStringLiteral("@define-color\\s+accent_(?:bg_)?color\\s+(#[0-9a-fA-F]{6})"));
    for (const char *p : {"gtk-4.0/dank-colors.css", "gtk-4.0/gtk.css", "gtk-4.0/colors.css", "gtk-3.0/gtk.css", "gtk-3.0/colors.css"}) {
        QFile f(config + u'/' + QLatin1String(p));
        if (!f.open(QIODevice::ReadOnly))
            continue;
        const auto m = accent.match(QString::fromUtf8(f.readAll()));
        if (m.hasMatch())
            if (const auto c = hex(m.captured(1)))
                return c;
    }
#ifdef RYNET_DBUS
    if (const auto v = portalRead(QStringLiteral("accent-color")); v && v->canConvert<QDBusArgument>()) {
        const QDBusArgument arg = v->value<QDBusArgument>();
        double r = -1, g = -1, b = -1;
        arg.beginStructure();
        arg >> r >> g >> b;
        arg.endStructure();
        if (r >= 0 && r <= 1 && g >= 0 && g <= 1 && b >= 0 && b <= 1)
            return QColor::fromRgbF(float(r), float(g), float(b));
    }
#endif
    const QString desktop = qEnvironmentVariable("XDG_CURRENT_DESKTOP").toLower();
    if (desktop.contains(QStringLiteral("kde"))) {
        QFile f(config + QStringLiteral("/kdeglobals"));
        if (f.open(QIODevice::ReadOnly))
            for (const QByteArray &line : f.readAll().split('\n'))
                if (line.startsWith("AccentColor=")) {
                    const QList<QByteArray> p = line.mid(12).split(',');
                    if (p.size() >= 3)
                        return QColor(p[0].trimmed().toInt(), p[1].trimmed().toInt(), p[2].trimmed().toInt());
                }
    }
    if (desktop.contains(QStringLiteral("gnome"))) {
        static const QHash<QString, quint32> gnome = {{"blue", 0x3584e4}, {"teal", 0x2190a4},   {"green", 0x3a944a}, {"yellow", 0xc88800}, {"orange", 0xed5b00},
                                                      {"red", 0xe62d42},  {"pink", 0xd56199},   {"purple", 0x9141ac}, {"slate", 0x6f8396}};
        if (const auto name = Platform::out(QStringLiteral("gsettings"), {QStringLiteral("get"), QStringLiteral("org.gnome.desktop.interface"), QStringLiteral("accent-color")})) {
            const QString n = QString(*name).remove(u'\'').trimmed();
            if (gnome.contains(n))
                return rgb(0xff000000u | gnome.value(n));
        }
    }
    return {};
#endif
}

}

Theme *Theme::instance()
{
    static Theme *t = new Theme;
    return t;
}

Theme *Theme::create(QQmlEngine *, QJSEngine *)
{
    QJSEngine::setObjectOwnership(instance(), QJSEngine::CppOwnership);
    return instance();
}

Theme::Theme(QObject *parent)
    : QObject(parent)
{
    m_poll.setInterval(Platform::windows() ? 15000 : 4000);
    connect(&m_poll, &QTimer::timeout, this, &Theme::detect);
    m_systemDark = systemDark();
    m_seed = systemSeed(m_systemDark);
    m_fade.setInterval(16);
    m_fade.setTimerType(Qt::PreciseTimer);
    m_ease.addCubicBezierSegment(QPointF(0.2, 0.0), QPointF(0.0, 1.0), QPointF(1.0, 1.0));
    connect(&m_fade, &QTimer::timeout, this, [this] {
        const double t = std::min(1.0, m_fadeClock.elapsed() / 320.0);
        const double k = m_ease.valueForProgress(t);
        for (int i = 0; i < Count; ++i)
            m_c[i] = t >= 1.0 ? m_to[i] : lerp(m_from[i], m_to[i], k);
        emit changed();
        if (t >= 1.0) {
            m_fade.stop();
            emit fadingChanged();
        }
    });
    recompute(true);
    listenPortal();
}

void Theme::listenPortal()
{
#ifdef RYNET_DBUS
    QDBusConnection::sessionBus().connect(QStringLiteral("org.freedesktop.portal.Desktop"), QStringLiteral("/org/freedesktop/portal/desktop"),
                                          QStringLiteral("org.freedesktop.portal.Settings"), QStringLiteral("SettingChanged"), this, SLOT(detect()));
#endif
}

void Theme::setWatching(bool on)
{
    if (on) {
        detect();
        m_poll.start();
    } else {
        m_poll.stop();
    }
}

void Theme::detect()
{
    if (m_detecting)
        return;
    m_detecting = true;
    using Result = std::pair<bool, std::optional<QColor>>;
    Platform::background<Result>(
        this,
        [] {
            const bool dark = systemDark();
            return Result(dark, systemSeed(dark));
        },
        [this](Result r) {
            m_detecting = false;
            apply(r.first, r.second);
        });
}

void Theme::apply(bool systemDark, std::optional<QColor> seed)
{
    if (systemDark == m_systemDark && seed == m_seed)
        return;
    m_systemDark = systemDark;
    m_seed = seed;
    recompute();
}

void Theme::setMode(const QString &mode, bool wallpaper, const QString &seed)
{
    const bool first = !m_shown;
    m_shown = true;
    const auto own = hex(seed);
    if (mode == m_mode && wallpaper == m_wallpaper && own == m_own)
        return;
    m_mode = mode;
    m_wallpaper = wallpaper;
    m_own = own;
    recompute(first);
}

Scheme Theme::brand(bool dark)
{
    Scheme s{};
    if (!dark) {
        set(s, {{Primary, 0xFF106B57},          {OnPrimary, 0xFFFFFFFF},          {PrimaryContainer, 0xFFA2F2D9},     {OnPrimaryContainer, 0xFF005141},
                {InversePrimary, 0xFF87D6BD},   {Secondary, 0xFF4B635B},          {OnSecondary, 0xFFFFFFFF},          {SecondaryContainer, 0xFFCDE9DE},
                {OnSecondaryContainer, 0xFF344C44}, {Tertiary, 0xFF416277},       {OnTertiary, 0xFFFFFFFF},           {TertiaryContainer, 0xFFC5E7FF},
                {OnTertiaryContainer, 0xFF284B5E}, {Background, 0xFFF5FBF6},      {OnBackground, 0xFF171D1B},         {Surface, 0xFFF5FBF6},
                {OnSurface, 0xFF171D1B},        {SurfaceVariant, 0xFFDBE5DF},     {OnSurfaceVariant, 0xFF3F4945},     {SurfaceTint, 0xFF106B57},
                {InverseSurface, 0xFF2B322F},   {InverseOnSurface, 0xFFECF2EE},   {Error, 0xFFBA1A1A},                {OnError, 0xFFFFFFFF},
                {ErrorContainer, 0xFFFFDAD6},   {OnErrorContainer, 0xFF93000A},   {Outline, 0xFF6F7975},              {OutlineVariant, 0xFFBFC9C4},
                {Scrim, 0xFF000000},            {SurfaceBright, 0xFFF5FBF6},      {SurfaceContainer, 0xFFE9EFEB},     {SurfaceContainerHigh, 0xFFE3EAE5},
                {SurfaceContainerHighest, 0xFFDEE4E0}, {SurfaceContainerLow, 0xFFEFF5F1}, {SurfaceContainerLowest, 0xFFFFFFFF}, {SurfaceDim, 0xFFD5DBD7},
                {PrimaryFixed, 0xFFA2F2D9},     {PrimaryFixedDim, 0xFF87D6BD},    {OnPrimaryFixed, 0xFF002019},       {OnPrimaryFixedVariant, 0xFF005141},
                {SecondaryFixed, 0xFFCDE9DE},   {SecondaryFixedDim, 0xFFB2CCC2},  {OnSecondaryFixed, 0xFF072019},     {OnSecondaryFixedVariant, 0xFF344C44},
                {TertiaryFixed, 0xFFC5E7FF},    {TertiaryFixedDim, 0xFFA9CBE2},   {OnTertiaryFixed, 0xFF001E2D},      {OnTertiaryFixedVariant, 0xFF284B5E}});
    } else {
        set(s, {{Primary, 0xFF87D6BD},          {OnPrimary, 0xFF00382C},          {PrimaryContainer, 0xFF005141},     {OnPrimaryContainer, 0xFFA2F2D9},
                {InversePrimary, 0xFF106B57},   {Secondary, 0xFFB2CCC2},          {OnSecondary, 0xFF1D352E},          {SecondaryContainer, 0xFF344C44},
                {OnSecondaryContainer, 0xFFCDE9DE}, {Tertiary, 0xFFA9CBE2},       {OnTertiary, 0xFF0E3446},           {TertiaryContainer, 0xFF284B5E},
                {OnTertiaryContainer, 0xFFC5E7FF}, {Background, 0xFF0F1512},      {OnBackground, 0xFFDEE4E0},         {Surface, 0xFF0F1512},
                {OnSurface, 0xFFDEE4E0},        {SurfaceVariant, 0xFF3F4945},     {OnSurfaceVariant, 0xFFBFC9C4},     {SurfaceTint, 0xFF87D6BD},
                {InverseSurface, 0xFFDEE4E0},   {InverseOnSurface, 0xFF2B322F},   {Error, 0xFFFFB4AB},                {OnError, 0xFF690005},
                {ErrorContainer, 0xFF93000A},   {OnErrorContainer, 0xFFFFDAD6},   {Outline, 0xFF89938E},              {OutlineVariant, 0xFF3F4945},
                {Scrim, 0xFF000000},            {SurfaceBright, 0xFF343B38},      {SurfaceContainer, 0xFF1B211E},     {SurfaceContainerHigh, 0xFF252B29},
                {SurfaceContainerHighest, 0xFF303633}, {SurfaceContainerLow, 0xFF171D1B}, {SurfaceContainerLowest, 0xFF090F0D}, {SurfaceDim, 0xFF0F1512},
                {PrimaryFixed, 0xFFA2F2D9},     {PrimaryFixedDim, 0xFF87D6BD},    {OnPrimaryFixed, 0xFF002019},       {OnPrimaryFixedVariant, 0xFF005141},
                {SecondaryFixed, 0xFFCDE9DE},   {SecondaryFixedDim, 0xFFB2CCC2},  {OnSecondaryFixed, 0xFF072019},     {OnSecondaryFixedVariant, 0xFF344C44},
                {TertiaryFixed, 0xFFC5E7FF},    {TertiaryFixedDim, 0xFFA9CBE2},   {OnTertiaryFixed, 0xFF001E2D},      {OnTertiaryFixedVariant, 0xFF284B5E}});
    }
    return s;
}

Scheme Theme::fromSeed(const QColor &seed, bool dark)
{
    static const std::array<int, std::size(Hct::kRoles)> index = [] {
        std::array<int, std::size(Hct::kRoles)> out{};
        for (size_t i = 0; i < out.size(); ++i)
            for (int r = 0; r < Count; ++r)
                if (!std::strcmp(names[r], Hct::kRoles[i].name))
                    out[i] = r;
        return out;
    }();
    const auto palettes = Hct::tonalSpot(seed.rgb());
    Scheme s = brand(dark);
    for (size_t i = 0; i < index.size(); ++i) {
        const Hct::RoleTone &r = Hct::kRoles[i];
        s[index[i]] = rgb(palettes[r.palette].tone(dark ? r.dark : r.light));
    }
    return s;
}

void Theme::recompute(bool snap)
{
    const bool dark = m_mode == u"dark" || m_mode == u"amoled" || (m_mode == u"system" && m_systemDark);
    const bool black = m_mode == u"amoled";
    Scheme s = (m_wallpaper && m_seed) ? fromSeed(*m_seed, dark) : m_own ? fromSeed(*m_own, dark) : brand(dark);
    if (black) {
        const QColor k(Qt::black);
        s[Background] = s[Surface] = s[SurfaceDim] = s[SurfaceContainerLowest] = k;
        s[SurfaceBright] = lerp(k, s[SurfaceBright], 0.8);
        s[SurfaceContainerLow] = lerp(k, s[SurfaceContainerLow], 0.6);
        s[SurfaceContainer] = lerp(k, s[SurfaceContainer], 0.7);
        s[SurfaceContainerHigh] = lerp(k, s[SurfaceContainerHigh], 0.78);
        s[SurfaceContainerHighest] = lerp(k, s[SurfaceContainerHighest], 0.85);
    }
    const quint32 warn[2][4] = {{0xFF725C00, 0xFFFFFFFF, 0xFFFFE082, 0xFF231B00}, {0xFFECC229, 0xFF3C2F00, 0xFF564500, 0xFFFFE082}};
    for (int i = 0; i < 4; ++i)
        s[Warning + i] = harmonize(rgb(warn[dark][i]), s[Primary]);
    if (s == m_to && dark == m_dark && black == m_amoled)
        return;
    m_to = s;
    m_dark = dark;
    m_amoled = black;
    if (snap) {
        const bool was = m_fade.isActive();
        m_fade.stop();
        m_c = s;
        emit changed();
        if (was)
            emit fadingChanged();
        return;
    }
    m_from = m_c;
    m_fadeClock.start();
    const bool was = m_fade.isActive();
    m_fade.start();
    if (!was)
        emit fadingChanged();
    emit changed();
}

QColor Theme::mix(const QColor &a, const QColor &b, qreal t) const { return lerp(a, b, std::clamp(t, 0.0, 1.0)); }

QVariantList Theme::swatch(const QString &seed, bool dark) const
{
    QVariantList out;
    if (const auto c = hex(seed)) {
        const auto p = Hct::tonalSpot(c->rgb());
        for (int i : {Hct::Primary, Hct::Secondary, Hct::Tertiary})
            out << QColor(rgb(p[i].tone(dark ? 80 : 40)));
    } else {
        const Scheme s = brand(dark);
        out << s[Primary] << s[Secondary] << s[Tertiary];
    }
    return out;
}

QColor Theme::hct(qreal hue, qreal chroma, qreal tone) const { return rgb(Hct::from(hue, chroma, tone)); }

qreal Theme::hueOf(const QColor &c) const { return Hct::hue(c.rgb()); }

QColor Theme::alpha(const QColor &c, qreal a) const
{
    QColor x = c;
    x.setAlphaF(float(std::clamp(a, 0.0, 1.0)));
    return x;
}

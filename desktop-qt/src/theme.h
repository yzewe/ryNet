#pragma once

#include <QColor>
#include <QObject>
#include <QQmlEngine>
#include <QTimer>
#include <QEasingCurve>
#include <QElapsedTimer>
#include <array>
#include <optional>

class QJSEngine;

namespace Role {
enum : int {
    Primary,
    OnPrimary,
    PrimaryContainer,
    OnPrimaryContainer,
    InversePrimary,
    Secondary,
    OnSecondary,
    SecondaryContainer,
    OnSecondaryContainer,
    Tertiary,
    OnTertiary,
    TertiaryContainer,
    OnTertiaryContainer,
    Background,
    OnBackground,
    Surface,
    OnSurface,
    SurfaceVariant,
    OnSurfaceVariant,
    SurfaceTint,
    InverseSurface,
    InverseOnSurface,
    Error,
    OnError,
    ErrorContainer,
    OnErrorContainer,
    Outline,
    OutlineVariant,
    Scrim,
    SurfaceBright,
    SurfaceDim,
    SurfaceContainerLowest,
    SurfaceContainerLow,
    SurfaceContainer,
    SurfaceContainerHigh,
    SurfaceContainerHighest,
    PrimaryFixed,
    PrimaryFixedDim,
    OnPrimaryFixed,
    OnPrimaryFixedVariant,
    SecondaryFixed,
    SecondaryFixedDim,
    OnSecondaryFixed,
    OnSecondaryFixedVariant,
    TertiaryFixed,
    TertiaryFixedDim,
    OnTertiaryFixed,
    OnTertiaryFixedVariant,
    Warning,
    OnWarning,
    WarningContainer,
    OnWarningContainer,
    Count
};
inline constexpr const char *names[] = {"primary", "onPrimary", "primaryContainer", "onPrimaryContainer", "inversePrimary", "secondary", "onSecondary", "secondaryContainer", "onSecondaryContainer", "tertiary", "onTertiary", "tertiaryContainer", "onTertiaryContainer", "background", "onBackground", "surface", "onSurface", "surfaceVariant", "onSurfaceVariant", "surfaceTint", "inverseSurface", "inverseOnSurface", "error", "onError", "errorContainer", "onErrorContainer", "outline", "outlineVariant", "scrim", "surfaceBright", "surfaceDim", "surfaceContainerLowest", "surfaceContainerLow", "surfaceContainer", "surfaceContainerHigh", "surfaceContainerHighest", "primaryFixed", "primaryFixedDim", "onPrimaryFixed", "onPrimaryFixedVariant", "secondaryFixed", "secondaryFixedDim", "onSecondaryFixed", "onSecondaryFixedVariant", "tertiaryFixed", "tertiaryFixedDim", "onTertiaryFixed", "onTertiaryFixedVariant", "warning", "onWarning", "warningContainer", "onWarningContainer"};
}

using Scheme = std::array<QColor, Role::Count>;

class Theme : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool dark READ dark NOTIFY changed)
    Q_PROPERTY(bool amoled READ amoled NOTIFY changed)
    Q_PROPERTY(bool systemColors READ systemColors NOTIFY changed)
    Q_PROPERTY(bool fading READ fading NOTIFY fadingChanged)
    Q_PROPERTY(QColor primary READ primary NOTIFY changed)
    Q_PROPERTY(QColor onPrimary READ onPrimary NOTIFY changed)
    Q_PROPERTY(QColor primaryContainer READ primaryContainer NOTIFY changed)
    Q_PROPERTY(QColor onPrimaryContainer READ onPrimaryContainer NOTIFY changed)
    Q_PROPERTY(QColor inversePrimary READ inversePrimary NOTIFY changed)
    Q_PROPERTY(QColor secondary READ secondary NOTIFY changed)
    Q_PROPERTY(QColor onSecondary READ onSecondary NOTIFY changed)
    Q_PROPERTY(QColor secondaryContainer READ secondaryContainer NOTIFY changed)
    Q_PROPERTY(QColor onSecondaryContainer READ onSecondaryContainer NOTIFY changed)
    Q_PROPERTY(QColor tertiary READ tertiary NOTIFY changed)
    Q_PROPERTY(QColor onTertiary READ onTertiary NOTIFY changed)
    Q_PROPERTY(QColor tertiaryContainer READ tertiaryContainer NOTIFY changed)
    Q_PROPERTY(QColor onTertiaryContainer READ onTertiaryContainer NOTIFY changed)
    Q_PROPERTY(QColor background READ background NOTIFY changed)
    Q_PROPERTY(QColor onBackground READ onBackground NOTIFY changed)
    Q_PROPERTY(QColor surface READ surface NOTIFY changed)
    Q_PROPERTY(QColor onSurface READ onSurface NOTIFY changed)
    Q_PROPERTY(QColor surfaceVariant READ surfaceVariant NOTIFY changed)
    Q_PROPERTY(QColor onSurfaceVariant READ onSurfaceVariant NOTIFY changed)
    Q_PROPERTY(QColor surfaceTint READ surfaceTint NOTIFY changed)
    Q_PROPERTY(QColor inverseSurface READ inverseSurface NOTIFY changed)
    Q_PROPERTY(QColor inverseOnSurface READ inverseOnSurface NOTIFY changed)
    Q_PROPERTY(QColor error READ error NOTIFY changed)
    Q_PROPERTY(QColor onError READ onError NOTIFY changed)
    Q_PROPERTY(QColor errorContainer READ errorContainer NOTIFY changed)
    Q_PROPERTY(QColor onErrorContainer READ onErrorContainer NOTIFY changed)
    Q_PROPERTY(QColor outline READ outline NOTIFY changed)
    Q_PROPERTY(QColor outlineVariant READ outlineVariant NOTIFY changed)
    Q_PROPERTY(QColor scrim READ scrim NOTIFY changed)
    Q_PROPERTY(QColor surfaceBright READ surfaceBright NOTIFY changed)
    Q_PROPERTY(QColor surfaceDim READ surfaceDim NOTIFY changed)
    Q_PROPERTY(QColor surfaceContainerLowest READ surfaceContainerLowest NOTIFY changed)
    Q_PROPERTY(QColor surfaceContainerLow READ surfaceContainerLow NOTIFY changed)
    Q_PROPERTY(QColor surfaceContainer READ surfaceContainer NOTIFY changed)
    Q_PROPERTY(QColor surfaceContainerHigh READ surfaceContainerHigh NOTIFY changed)
    Q_PROPERTY(QColor surfaceContainerHighest READ surfaceContainerHighest NOTIFY changed)
    Q_PROPERTY(QColor primaryFixed READ primaryFixed NOTIFY changed)
    Q_PROPERTY(QColor primaryFixedDim READ primaryFixedDim NOTIFY changed)
    Q_PROPERTY(QColor onPrimaryFixed READ onPrimaryFixed NOTIFY changed)
    Q_PROPERTY(QColor onPrimaryFixedVariant READ onPrimaryFixedVariant NOTIFY changed)
    Q_PROPERTY(QColor secondaryFixed READ secondaryFixed NOTIFY changed)
    Q_PROPERTY(QColor secondaryFixedDim READ secondaryFixedDim NOTIFY changed)
    Q_PROPERTY(QColor onSecondaryFixed READ onSecondaryFixed NOTIFY changed)
    Q_PROPERTY(QColor onSecondaryFixedVariant READ onSecondaryFixedVariant NOTIFY changed)
    Q_PROPERTY(QColor tertiaryFixed READ tertiaryFixed NOTIFY changed)
    Q_PROPERTY(QColor tertiaryFixedDim READ tertiaryFixedDim NOTIFY changed)
    Q_PROPERTY(QColor onTertiaryFixed READ onTertiaryFixed NOTIFY changed)
    Q_PROPERTY(QColor onTertiaryFixedVariant READ onTertiaryFixedVariant NOTIFY changed)
    Q_PROPERTY(QColor warning READ warning NOTIFY changed)
    Q_PROPERTY(QColor onWarning READ onWarning NOTIFY changed)
    Q_PROPERTY(QColor warningContainer READ warningContainer NOTIFY changed)
    Q_PROPERTY(QColor onWarningContainer READ onWarningContainer NOTIFY changed)

public:
    static Theme *instance();
    static Theme *create(QQmlEngine *, QJSEngine *);

    bool dark() const { return m_dark; }
    bool amoled() const { return m_amoled; }
    bool systemColors() const { return m_wallpaper && m_seed.has_value(); }
    bool fading() const { return m_fade.isActive(); }
    QColor primary() const { return m_c[Role::Primary]; }
    QColor onPrimary() const { return m_c[Role::OnPrimary]; }
    QColor primaryContainer() const { return m_c[Role::PrimaryContainer]; }
    QColor onPrimaryContainer() const { return m_c[Role::OnPrimaryContainer]; }
    QColor inversePrimary() const { return m_c[Role::InversePrimary]; }
    QColor secondary() const { return m_c[Role::Secondary]; }
    QColor onSecondary() const { return m_c[Role::OnSecondary]; }
    QColor secondaryContainer() const { return m_c[Role::SecondaryContainer]; }
    QColor onSecondaryContainer() const { return m_c[Role::OnSecondaryContainer]; }
    QColor tertiary() const { return m_c[Role::Tertiary]; }
    QColor onTertiary() const { return m_c[Role::OnTertiary]; }
    QColor tertiaryContainer() const { return m_c[Role::TertiaryContainer]; }
    QColor onTertiaryContainer() const { return m_c[Role::OnTertiaryContainer]; }
    QColor background() const { return m_c[Role::Background]; }
    QColor onBackground() const { return m_c[Role::OnBackground]; }
    QColor surface() const { return m_c[Role::Surface]; }
    QColor onSurface() const { return m_c[Role::OnSurface]; }
    QColor surfaceVariant() const { return m_c[Role::SurfaceVariant]; }
    QColor onSurfaceVariant() const { return m_c[Role::OnSurfaceVariant]; }
    QColor surfaceTint() const { return m_c[Role::SurfaceTint]; }
    QColor inverseSurface() const { return m_c[Role::InverseSurface]; }
    QColor inverseOnSurface() const { return m_c[Role::InverseOnSurface]; }
    QColor error() const { return m_c[Role::Error]; }
    QColor onError() const { return m_c[Role::OnError]; }
    QColor errorContainer() const { return m_c[Role::ErrorContainer]; }
    QColor onErrorContainer() const { return m_c[Role::OnErrorContainer]; }
    QColor outline() const { return m_c[Role::Outline]; }
    QColor outlineVariant() const { return m_c[Role::OutlineVariant]; }
    QColor scrim() const { return m_c[Role::Scrim]; }
    QColor surfaceBright() const { return m_c[Role::SurfaceBright]; }
    QColor surfaceDim() const { return m_c[Role::SurfaceDim]; }
    QColor surfaceContainerLowest() const { return m_c[Role::SurfaceContainerLowest]; }
    QColor surfaceContainerLow() const { return m_c[Role::SurfaceContainerLow]; }
    QColor surfaceContainer() const { return m_c[Role::SurfaceContainer]; }
    QColor surfaceContainerHigh() const { return m_c[Role::SurfaceContainerHigh]; }
    QColor surfaceContainerHighest() const { return m_c[Role::SurfaceContainerHighest]; }
    QColor primaryFixed() const { return m_c[Role::PrimaryFixed]; }
    QColor primaryFixedDim() const { return m_c[Role::PrimaryFixedDim]; }
    QColor onPrimaryFixed() const { return m_c[Role::OnPrimaryFixed]; }
    QColor onPrimaryFixedVariant() const { return m_c[Role::OnPrimaryFixedVariant]; }
    QColor secondaryFixed() const { return m_c[Role::SecondaryFixed]; }
    QColor secondaryFixedDim() const { return m_c[Role::SecondaryFixedDim]; }
    QColor onSecondaryFixed() const { return m_c[Role::OnSecondaryFixed]; }
    QColor onSecondaryFixedVariant() const { return m_c[Role::OnSecondaryFixedVariant]; }
    QColor tertiaryFixed() const { return m_c[Role::TertiaryFixed]; }
    QColor tertiaryFixedDim() const { return m_c[Role::TertiaryFixedDim]; }
    QColor onTertiaryFixed() const { return m_c[Role::OnTertiaryFixed]; }
    QColor onTertiaryFixedVariant() const { return m_c[Role::OnTertiaryFixedVariant]; }
    QColor warning() const { return m_c[Role::Warning]; }
    QColor onWarning() const { return m_c[Role::OnWarning]; }
    QColor warningContainer() const { return m_c[Role::WarningContainer]; }
    QColor onWarningContainer() const { return m_c[Role::OnWarningContainer]; }

    void setMode(const QString &mode, bool wallpaper, const QString &seed);
    void setWatching(bool on);
    QColor color(int role) const { return m_c[role]; }

    Q_INVOKABLE QColor mix(const QColor &a, const QColor &b, qreal t) const;
    Q_INVOKABLE QColor alpha(const QColor &c, qreal a) const;
    Q_INVOKABLE QVariantList swatch(const QString &seed, bool dark) const;
    Q_INVOKABLE QColor hct(qreal hue, qreal chroma, qreal tone) const;
    Q_INVOKABLE qreal hueOf(const QColor &c) const;

    static Scheme brand(bool dark);
    static Scheme fromSeed(const QColor &seed, bool dark);

signals:
    void changed();
    void fadingChanged();

private slots:
    void detect();

private:
    explicit Theme(QObject *parent = nullptr);
    void apply(bool systemDark, std::optional<QColor> seed);
    void recompute(bool snap = false);
    void listenPortal();

    Scheme m_c{};
    bool m_dark = false;
    bool m_amoled = false;
    QString m_mode = QStringLiteral("system");
    bool m_wallpaper = true;
    bool m_systemDark = false;
    std::optional<QColor> m_seed;
    std::optional<QColor> m_own;
    Scheme m_from{}, m_to{};
    bool m_shown = false;
    QTimer m_fade;
    QElapsedTimer m_fadeClock;
    QEasingCurve m_ease{QEasingCurve::BezierSpline};
    bool m_detecting = false;
    QTimer m_poll;
};

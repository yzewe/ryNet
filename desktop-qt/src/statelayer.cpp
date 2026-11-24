#include "statelayer.h"

#include "theme.h"

#include <QQuickWindow>
#include <QSGGeometryNode>
#include <QSGMaterial>
#include <QSGMaterialShader>
#include <QSGRendererInterface>
#include <cmath>
#include <cstring>

namespace {
const QEasingCurve &decelerate()
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.05, 0.7), QPointF(0.1, 1), QPointF(1, 1));
        return e;
    }();
    return c;
}
const QEasingCurve &outCubic()
{
    static const QEasingCurve c(QEasingCurve::OutCubic);
    return c;
}

struct Uniforms {
    float tint[4];
    float ringColor[4];
    float radii[4];
    float size[2];
    float center[2];
    float ripple, rippleAlpha, stateAlpha, inset, ring;
};

class StateMaterial : public QSGMaterial {
public:
    StateMaterial() { setFlag(Blending); }
    QSGMaterialType *type() const override
    {
        static QSGMaterialType t;
        return &t;
    }
    QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override;
    int compare(const QSGMaterial *other) const override
    {
        return std::memcmp(&u, &static_cast<const StateMaterial *>(other)->u, sizeof(Uniforms));
    }
    Uniforms u = {};
};

class StateShader : public QSGMaterialShader {
public:
    StateShader()
    {
        setShaderFileName(VertexStage, QStringLiteral(":/state.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(":/state.frag.qsb"));
    }
    bool updateUniformData(RenderState &state, QSGMaterial *next, QSGMaterial *) override
    {
        QByteArray *buf = state.uniformData();
        if (state.isMatrixDirty()) {
            const QMatrix4x4 m = state.combinedMatrix();
            std::memcpy(buf->data(), m.constData(), 64);
        }
        if (state.isOpacityDirty()) {
            const float o = state.opacity();
            std::memcpy(buf->data() + 64, &o, 4);
        }
        std::memcpy(buf->data() + 80, &static_cast<StateMaterial *>(next)->u, sizeof(Uniforms));
        return true;
    }
};

QSGMaterialShader *StateMaterial::createShader(QSGRendererInterface::RenderMode) const { return new StateShader; }

void put(float *to, const QColor &c)
{
    to[0] = float(c.redF());
    to[1] = float(c.greenF());
    to[2] = float(c.blueF());
    to[3] = float(c.alphaF());
}
}

double StateLayer::Tween::at(qint64 now, double current)
{
    if (start < 0)
        return current;
    const double t = ms > 0 ? std::min(1.0, double(now - start) / ms) : 1.0;
    if (t >= 1.0) {
        start = -1;
        return to;
    }
    return from + (to - from) * curve->valueForProgress(t);
}

StateLayer::StateLayer(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents);
    m_ring.stiffness = 800;
    m_ring.damping = 1;
    m_clock.start();
    connect(Theme::instance(), &Theme::changed, this, [this] {
        if (shown())
            update();
    });
}

QColor StateLayer::tint() const { return m_tintSet ? m_tint : Theme::instance()->onSurface(); }

void StateLayer::setTint(const QColor &c)
{
    if (m_tintSet && c == m_tint)
        return;
    if (m_complete && m_fadeTint && m_tintSet && !Theme::instance()->fading()) {
        m_tintFrom = drawnTint();
        m_tintTween.run(0, 1, 180, &outCubic(), m_clock.elapsed());
        wake();
    } else {
        m_tintTween.start = -1;
    }
    m_tint = c;
    m_tintSet = true;
    emit tintChanged();
    if (shown())
        update();
}

void StateLayer::setFadeTint(bool on)
{
    if (on == m_fadeTint)
        return;
    m_fadeTint = on;
    emit fadeTintChanged();
}

QColor StateLayer::drawnTint() const
{
    const QColor to = tint();
    if (!m_tintTween.running())
        return to;
    const double k = m_tintMix;
    auto mix = [k](double a, double b) { return a + (b - a) * k; };
    return QColor::fromRgbF(float(mix(m_tintFrom.redF(), to.redF())), float(mix(m_tintFrom.greenF(), to.greenF())),
                            float(mix(m_tintFrom.blueF(), to.blueF())), float(mix(m_tintFrom.alphaF(), to.alphaF())));
}

void StateLayer::setRadii(const QVector4D &r)
{
    if (r == m_radii)
        return;
    m_radii = r;
    emit radiiChanged();
    if (shown())
        update();
}

void StateLayer::setHovered(bool on)
{
    if (on == m_hovered)
        return;
    m_hovered = on;
    emit hoveredChanged();
    aim();
}

void StateLayer::setFocused(bool on)
{
    if (on == m_focused)
        return;
    m_focused = on;
    emit focusedChanged();
    aim();
}

void StateLayer::setFocusRing(bool on)
{
    if (on == m_focusRing)
        return;
    m_focusRing = on;
    emit focusRingChanged();
    if (!m_originSet)
        emit originChanged();
    aim();
}

QPointF StateLayer::origin() const { return m_originSet ? m_origin : QPointF(shapeWidth() / 2, shapeHeight() / 2); }

void StateLayer::setOrigin(const QPointF &p)
{
    if (m_originSet && p == m_origin)
        return;
    m_origin = p;
    m_originSet = true;
    emit originChanged();
}

void StateLayer::setStateAlpha(qreal a)
{
    m_alphaFixed = true;
    m_alphaTween.start = -1;
    if (a == m_alpha)
        return;
    m_alpha = a;
    emit stateAlphaChanged();
    update();
}

void StateLayer::setPressed(bool on)
{
    if (on == m_pressed)
        return;
    m_pressed = on;
    emit pressedChanged();
    if (!m_complete)
        return;
    const qint64 now = m_clock.elapsed();
    if (on) {
        m_fadeAt = -1;
        m_rippleFade.start = -1;
        m_center = origin() + QPointF(inset(), inset());
        m_pressedAt = now;
        m_ripple = reach() * 0.12;
        m_rippleAlpha = 0.10;
        m_rippleTween.run(m_ripple, reach(), 360, &decelerate(), now);
        update();
    } else {
        m_fadeAt = now + std::max<qint64>(1, 90 - (now - m_pressedAt));
    }
    wake();
}

double StateLayer::reach() const
{
    const double cx = m_center.x() - inset(), cy = m_center.y() - inset();
    const double dx = std::max(cx, shapeWidth() - cx), dy = std::max(cy, shapeHeight() - cy);
    return std::sqrt(dx * dx + dy * dy) * 1.25 + 8;
}

void StateLayer::aim()
{
    const double alpha = m_focused ? 0.10 : m_hovered ? 0.08 : 0;
    const double ring = m_focusRing && m_focused ? 1 : 0;
    if (!m_complete) {
        if (!m_alphaFixed)
            m_alpha = alpha;
        m_ring.value = m_ring.target = ring;
        return;
    }
    const qint64 now = m_clock.elapsed();
    if (!m_alphaFixed && alpha != (m_alphaTween.running() ? m_alphaTween.to : m_alpha))
        m_alphaTween.run(m_alpha, alpha, 140, &outCubic(), now);
    m_ring.damping = m_focused ? 0.6 : 1;
    m_ring.target = ring;
    wake();
}

void StateLayer::componentComplete()
{
    QQuickItem::componentComplete();
    m_complete = true;
    m_center = QPointF(width() / 2, height() / 2);
}

void StateLayer::geometryChange(const QRectF &next, const QRectF &old)
{
    QQuickItem::geometryChange(next, old);
    if (!m_pressed && m_rippleAlpha <= 0)
        m_center = QPointF(next.width() / 2, next.height() / 2);
    if (!m_originSet && next.size() != old.size())
        emit originChanged();
    if (shown())
        update();
}

bool StateLayer::shown() const { return m_alpha > 0.002 || m_rippleAlpha > 0.002 || m_ring.value > 0.005; }

void StateLayer::wake()
{
    if (!m_driver)
        m_driver = new SpringClock([this](double) { return tick(); }, this);
    if (m_driver->state() != QAbstractAnimation::Running) {
        m_lastTick = -1;
        m_driver->start();
    }
}

bool StateLayer::tick()
{
    const qint64 now = m_clock.elapsed();
    const double dt = m_lastTick < 0 ? 0.0 : std::min(0.064, (now - m_lastTick) / 1000.0);
    m_lastTick = now;
    const bool before = shown();
    if (m_fadeAt >= 0 && now >= m_fadeAt) {
        m_fadeAt = -1;
        m_rippleTween.run(m_ripple, reach(), 260, &outCubic(), now);
        m_rippleFade.run(m_rippleAlpha, 0, 420, &decelerate(), now);
    }
    m_tintMix = m_tintTween.at(now, 1);
    const double alpha = m_alphaTween.at(now, m_alpha);
    if (alpha != m_alpha) {
        m_alpha = alpha;
        emit stateAlphaChanged();
    }
    m_ripple = m_rippleTween.at(now, m_ripple);
    m_rippleAlpha = m_rippleFade.at(now, m_rippleAlpha);
    if (!m_ring.settled() || m_ring.value != m_ring.target)
        m_ring.step(dt);
    const bool rest = advance(dt);
    if (before || shown())
        update();
    return rest && !m_alphaTween.running() && !m_tintTween.running() && !m_rippleTween.running() && !m_rippleFade.running() && m_fadeAt < 0
        && m_ring.settled() && m_ring.value == m_ring.target;
}

QSGNode *StateLayer::updatePaintNode(QSGNode *old, UpdatePaintNodeData *)
{
    auto *node = static_cast<QSGGeometryNode *>(old);
    if (!shown() || width() <= 0 || height() <= 0
        || window()->rendererInterface()->graphicsApi() == QSGRendererInterface::Software) {
        delete node;
        return nullptr;
    }
    if (!node) {
        node = new QSGGeometryNode;
        auto *g = new QSGGeometry(QSGGeometry::defaultAttributes_TexturedPoint2D(), 4);
        node->setGeometry(g);
        node->setFlag(QSGNode::OwnsGeometry);
        node->setMaterial(new StateMaterial);
        node->setFlag(QSGNode::OwnsMaterial);
    }
    QSGGeometry::updateTexturedRectGeometry(node->geometry(), boundingRect(), QRectF(0, 0, 1, 1));
    node->markDirty(QSGNode::DirtyGeometry);
    Uniforms &u = static_cast<StateMaterial *>(node->material())->u;
    put(u.tint, drawnTint());
    put(u.ringColor, Theme::instance()->secondary());
    u.radii[0] = m_radii.x();
    u.radii[1] = m_radii.y();
    u.radii[2] = m_radii.z();
    u.radii[3] = m_radii.w();
    u.size[0] = float(width());
    u.size[1] = float(height());
    u.center[0] = float(m_center.x());
    u.center[1] = float(m_center.y());
    u.ripple = float(m_ripple);
    u.rippleAlpha = float(m_rippleAlpha);
    u.stateAlpha = float(m_alpha);
    u.inset = float(inset());
    u.ring = float(m_ring.value);
    node->markDirty(QSGNode::DirtyMaterial);
    return node;
}

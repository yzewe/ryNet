#include "items.h"

#include "shapes_data.h"

#include <QDateTime>
#include <QEasingCurve>
#include <QPainter>
#include <QPainterPath>
#include <QPointer>
#include <QQuickWindow>
#include <QSGGeometryNode>
#include <QSGImageNode>
#include <QSGRendererInterface>
#include <QSGVertexColorMaterial>
#include <cmath>
#include <qrencode.h>
#include <vector>

namespace {
constexpr double kTau = 6.283185307179586;

class MotionVisibility : public QObject {
public:
    MotionVisibility(QQuickItem *item, std::function<void()> sync)
        : QObject(item), m_sync(std::move(sync))
    {
        connect(item, &QQuickItem::visibleChanged, this, [this] { m_sync(); });
        connect(item, &QQuickItem::windowChanged, this, [this](QQuickWindow *window) { watch(window); });
        watch(item->window());
    }

protected:
    bool eventFilter(QObject *object, QEvent *event) override
    {
        if (object == m_window && (event->type() == QEvent::Expose || event->type() == QEvent::Show
                                  || event->type() == QEvent::Hide || event->type() == QEvent::WindowStateChange))
            m_sync();
        return false;
    }

private:
    void watch(QQuickWindow *window)
    {
        if (m_window)
            m_window->removeEventFilter(this);
        m_window = window;
        if (m_window)
            m_window->installEventFilter(this);
        m_sync();
    }
    std::function<void()> m_sync;
    QPointer<QQuickWindow> m_window;
};

bool motionVisible(const QQuickItem *item)
{
    const auto *window = item->window();
    return item->isVisible() && window && window->isVisible() && window->isExposed()
           && !(window->windowStates() & Qt::WindowMinimized);
}

const shapes_data::Entry *findShape(const QString &name)
{
    for (const auto &e : shapes_data::kShapes)
        if (name == QLatin1String(e.name))
            return &e;
    return nullptr;
}

const shapes_data::MorphEntry *findMorph(const QString &from, const QString &to)
{
    for (const auto &m : shapes_data::kMorphs)
        if (from == QLatin1String(m.from) && to == QLatin1String(m.to))
            return &m;
    return nullptr;
}

QPainterPath cubicPath(const float *pts, int n, const QRectF &r)
{
    QPainterPath path;
    auto pt = [&](float x, float y) { return QPointF(r.x() + x * r.width(), r.y() + y * r.height()); };
    for (int i = 0; i < n; ++i) {
        const float *c = pts + i * 8;
        if (i == 0)
            path.moveTo(pt(c[0], c[1]));
        path.cubicTo(pt(c[2], c[3]), pt(c[4], c[5]), pt(c[6], c[7]));
    }
    path.closeSubpath();
    return path;
}

QPainterPath morphPath(const shapes_data::MorphEntry *m, double t, const QRectF &r)
{
    std::vector<float> pts(size_t(m->count) * 8);
    for (size_t i = 0; i < pts.size(); ++i)
        pts[i] = float(m->start[i] + (m->end[i] - m->start[i]) * t);
    return cubicPath(pts.data(), m->count, r);
}

QPainterPath shapePath(const QString &name, const QRectF &r)
{
    const auto *e = findShape(name);
    if (!e)
        e = findShape(QStringLiteral("Circle"));
    return cubicPath(e->points, e->count, r);
}

QPainterPath shapeOrMorph(const QString &from, const QString &to, double t, const QRectF &r)
{
    if (!to.isEmpty() && to != from && t > 0) {
        if (t >= 1.0 && std::abs(t - 1.0) < 1e-4)
            return shapePath(to, r);
        if (const auto *m = findMorph(from, to))
            return morphPath(m, t, r);
        return shapePath(t < 0.5 ? from : to, r);
    }
    return shapePath(from, r);
}

double smoothstep01(double v)
{
    const double u = std::clamp(v, 0.0, 1.0);
    return u * u * (3 - 2 * u);
}

double easeDecelerate(double t)
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.05, 0.7), QPointF(0.1, 1.0), QPointF(1, 1));
        return e;
    }();
    return c.valueForProgress(std::clamp(t, 0.0, 1.0));
}

double easeStandard(double t)
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.4, 0.0), QPointF(0.2, 1.0), QPointF(1, 1));
        return e;
    }();
    return c.valueForProgress(std::clamp(t, 0.0, 1.0));
}

const float kLogo[] = {
    330, 540, 400, 560, 452, 560, 446, 620, 440, 676, 360, 700, 330, 760, 300, 820, 330, 856, 420, 862, 520, 870, 640, 864,
    712, 822, 782, 780, 772, 700, 700, 668, 640, 640, 610, 600, 622, 548, 634, 496, 730, 470, 724, 380, 718, 280, 620, 214,
    512, 214, 400, 214, 330, 260, 312, 330, 302, 372, 298, 400, 300, 430,
};

struct Logo {
    static constexpr int N = 360;
    float x[N], y[N], s[N], nx[N], ny[N];
    float stroke = 0;
    int loop = 0;
    std::vector<float> lx, ly, lnx, lny;
    float step = 0;

    static std::pair<float, float> norm(float x, float y)
    {
        const float l = std::max(std::hypot(x, y), 1e-6f);
        return {x / l, y / l};
    }

    Logo()
    {
        const int steps = 48;
        const int segs = (int(sizeof(kLogo) / sizeof(float)) - 2) / 6;
        const int m = segs * steps + 1;
        std::vector<float> dx(static_cast<size_t>(m)), dy(static_cast<size_t>(m)), len(static_cast<size_t>(m));
        dx[0] = kLogo[0];
        dy[0] = kLogo[1];
        for (int sg = 0; sg < segs; ++sg) {
            const int o = 2 + sg * 6;
            for (int j = 1; j <= steps; ++j) {
                const float t = float(j) / steps, u = 1 - t;
                const float a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t;
                dx[size_t(sg * steps + j)] = a * kLogo[o - 2] + b * kLogo[o] + c * kLogo[o + 2] + d * kLogo[o + 4];
                dy[size_t(sg * steps + j)] = a * kLogo[o - 1] + b * kLogo[o + 1] + c * kLogo[o + 3] + d * kLogo[o + 5];
            }
        }
        for (int k = 1; k < m; ++k)
            len[size_t(k)] = len[size_t(k - 1)] + std::hypot(dx[size_t(k)] - dx[size_t(k - 1)], dy[size_t(k)] - dy[size_t(k - 1)]);
        const float h = *std::max_element(dy.begin(), dy.end()) - *std::min_element(dy.begin(), dy.end());
        int k = 0;
        for (int i = 0; i < N; ++i) {
            const float at = len[size_t(m - 1)] * i / (N - 1);
            while (k < m - 2 && len[size_t(k + 1)] < at)
                ++k;
            const float f = std::clamp((at - len[size_t(k)]) / (len[size_t(k + 1)] - len[size_t(k)]), 0.0f, 1.0f);
            x[i] = (dx[size_t(k)] + (dx[size_t(k + 1)] - dx[size_t(k)]) * f - 512.0f) / h;
            y[i] = (dy[size_t(k)] + (dy[size_t(k + 1)] - dy[size_t(k)]) * f - 538.0f) / h;
            s[i] = at / h;
        }
        for (int i = 0; i < N; ++i) {
            const float tx = x[std::min(i + 1, N - 1)] - x[std::max(i - 1, 0)];
            const float ty = y[std::min(i + 1, N - 1)] - y[std::max(i - 1, 0)];
            const float l = std::hypot(tx, ty);
            nx[i] = -ty / l;
            ny[i] = tx / l;
        }
        stroke = 68.0f / h;
        const float ds = s[1] - s[0];
        const float ex = x[N - 1], ey = y[N - 1], sx = x[0], sy = y[0];
        const float gap = std::hypot(sx - ex, sy - ey);
        const auto te = norm(x[N - 1] - x[N - 2], y[N - 1] - y[N - 2]);
        const auto ts = norm(x[1] - x[0], y[1] - y[0]);
        const float k1 = gap * 0.6f;
        const float c1x = ex + te.first * k1, c1y = ey + te.second * k1;
        const float c2x = sx - ts.first * k1, c2y = sy - ts.second * k1;
        const int fine = 200;
        std::vector<float> bx(fine + 1), by(fine + 1), bl(fine + 1);
        for (int j = 0; j <= fine; ++j) {
            const float t = float(j) / fine, u = 1 - t;
            bx[size_t(j)] = u * u * u * ex + 3 * u * u * t * c1x + 3 * u * t * t * c2x + t * t * t * sx;
            by[size_t(j)] = u * u * u * ey + 3 * u * u * t * c1y + 3 * u * t * t * c2y + t * t * t * sy;
            if (j > 0)
                bl[size_t(j)] = bl[size_t(j - 1)] + std::hypot(bx[size_t(j)] - bx[size_t(j - 1)], by[size_t(j)] - by[size_t(j - 1)]);
        }
        const int bridge = std::max(1, int(bl[fine] / ds));
        loop = N + bridge - 1;
        lx.assign(x, x + N);
        ly.assign(y, y + N);
        lx.resize(size_t(loop));
        ly.resize(size_t(loop));
        int q = 0;
        for (int b = 1; b < bridge; ++b) {
            const float at = bl[fine] * b / bridge;
            while (q < fine - 1 && bl[size_t(q + 1)] < at)
                ++q;
            const float f = std::clamp((at - bl[size_t(q)]) / (bl[size_t(q + 1)] - bl[size_t(q)]), 0.0f, 1.0f);
            lx[size_t(N - 1 + b)] = bx[size_t(q)] + (bx[size_t(q + 1)] - bx[size_t(q)]) * f;
            ly[size_t(N - 1 + b)] = by[size_t(q)] + (by[size_t(q + 1)] - by[size_t(q)]) * f;
        }
        lnx.resize(size_t(loop));
        lny.resize(size_t(loop));
        for (int i = 0; i < loop; ++i) {
            const auto n = norm(lx[size_t((i + 1) % loop)] - lx[size_t((i + loop - 1) % loop)], ly[size_t((i + 1) % loop)] - ly[size_t((i + loop - 1) % loop)]);
            lnx[size_t(i)] = -n.second;
            lny[size_t(i)] = n.first;
        }
        step = ds;
    }
};

const Logo &logo()
{
    static const Logo l;
    return l;
}

QPainterPath smoothOpen(const float *xs, const float *ys, int n)
{
    QPainterPath p;
    if (n < 2)
        return p;
    p.moveTo(xs[0], ys[0]);
    for (int i = 1; i < n - 1; ++i)
        p.quadTo(xs[i], ys[i], (xs[i] + xs[i + 1]) / 2, (ys[i] + ys[i + 1]) / 2);
    p.lineTo(xs[n - 1], ys[n - 1]);
    return p;
}

QPen roundPen(const QColor &c, double w)
{
    QPen pen(c, w);
    pen.setCapStyle(Qt::RoundCap);
    pen.setJoinStyle(Qt::RoundJoin);
    return pen;
}

QColor withAlpha(QColor c, double a)
{
    c.setAlphaF(float(std::clamp(a, 0.0, 1.0) * c.alphaF()));
    return c;
}

constexpr double kField = 260.0 / 188.0;
constexpr double kLogoH = 0.44;
constexpr double kThin = 0.42;

QString statusShape(int s)
{
    switch (s) {
    case 1:
        return QStringLiteral("Cookie6Sided");
    case 2:
        return QStringLiteral("Circle");
    case 3:
        return QStringLiteral("Clover4Leaf");
    default:
        return QStringLiteral("Cookie9Sided");
    }
}
}

MShape::MShape(QQuickItem *parent)
    : QQuickPaintedItem(parent)
{
    setAntialiasing(true);
}

bool MShape::exists(const QString &name) { return findShape(name) != nullptr; }

void MShape::setShape(const QString &s)
{
    if (s == m_shape)
        return;
    m_shape = s;
    emit changed();
    update();
}

void MShape::setMorphTo(const QString &s)
{
    if (s == m_morphTo)
        return;
    m_morphTo = s;
    emit changed();
    update();
}

void MShape::setProgress(qreal v)
{
    if (qFuzzyCompare(v, m_progress))
        return;
    m_progress = v;
    emit changed();
    update();
}

void MShape::setColor(const QColor &c)
{
    if (c == m_color)
        return;
    m_color = c;
    emit changed();
    update();
}

void MShape::paint(QPainter *p)
{
    p->setRenderHint(QPainter::Antialiasing);
    p->setPen(Qt::NoPen);
    p->setBrush(m_color);
    p->drawPath(shapeOrMorph(m_shape, m_morphTo, m_progress, QRectF(0, 0, width(), height())));
}

namespace {
struct SpinnerPose {
    const char *from, *to;
    double progress, rotation, scale;
};

SpinnerPose spinnerPose(double t, bool contained)
{
    static const char *const seq[] = {"SoftBurst", "Cookie9Sided", "Pentagon", "Pill", "Sunny", "Cookie4Sided", "Oval"};
    const double period = 0.65;
    const int stepIndex = int(std::floor(t / period));
    const double local = t - stepIndex * period;
    const double w0 = std::sqrt(200.0), z = 0.6, wd = w0 * std::sqrt(1 - z * z);
    const double progress = 1 - std::exp(-z * w0 * local) * (std::cos(wd * local) + z * w0 / wd * std::sin(wd * local));
    const double rotation = std::fmod(t / 4.666 * 360.0 + stepIndex * 90.0 + progress * 90.0, 360.0);
    return {seq[stepIndex % 7], seq[(stepIndex + 1) % 7], std::max(1e-3, progress), rotation, contained ? 0.68 : 38.0 / 48.0};
}

std::vector<float> spinnerCubics(const SpinnerPose &pose, int *count)
{
    const QString from = QLatin1String(pose.from), to = QLatin1String(pose.to);
    if (const auto *m = findMorph(from, to)) {
        std::vector<float> pts(size_t(m->count) * 8);
        for (size_t i = 0; i < pts.size(); ++i)
            pts[i] = float(m->start[i] + (m->end[i] - m->start[i]) * pose.progress);
        *count = m->count;
        return pts;
    }
    const auto *e = findShape(pose.progress < 0.5 ? from : to);
    *count = e->count;
    return std::vector<float>(e->points, e->points + size_t(e->count) * 8);
}

void fanWithFringe(std::vector<QSGGeometry::ColoredPoint2D> &v, std::vector<quint32> &idx, const std::vector<QPointF> &outline,
                   QPointF centre, const QColor &color, double fringe)
{
    const int n = int(outline.size());
    if (n < 3)
        return;
    const float a = float(color.alphaF());
    const uchar r = uchar(color.redF() * a * 255 + 0.5), g = uchar(color.greenF() * a * 255 + 0.5),
                b = uchar(color.blueF() * a * 255 + 0.5), al = uchar(a * 255 + 0.5);
    double area = 0;
    for (int i = 0; i < n; ++i) {
        const QPointF &p = outline[i], &q = outline[(i + 1) % n];
        area += p.x() * q.y() - q.x() * p.y();
    }
    const double out = area > 0 ? 1 : -1;
    const quint32 base = quint32(v.size());
    v.push_back({float(centre.x()), float(centre.y()), r, g, b, al});
    for (int i = 0; i < n; ++i) {
        const QPointF &prev = outline[(i + n - 1) % n], &p = outline[i], &next = outline[(i + 1) % n];
        const QPointF d1 = p - prev, d2 = next - p;
        QPointF nrm(d1.y() + d2.y(), -(d1.x() + d2.x()));
        const double len = std::hypot(nrm.x(), nrm.y());
        nrm = len > 1e-9 ? nrm * (out * fringe / 2 / len) : QPointF();
        v.push_back({float(p.x() - nrm.x()), float(p.y() - nrm.y()), r, g, b, al});
        v.push_back({float(p.x() + nrm.x()), float(p.y() + nrm.y()), 0, 0, 0, 0});
    }
    for (int i = 0; i < n; ++i) {
        const quint32 in0 = base + 1 + 2 * i, out0 = in0 + 1, in1 = base + 1 + 2 * ((i + 1) % n), out1 = in1 + 1;
        idx.insert(idx.end(), {base, in0, in1, in0, out0, in1, in1, out0, out1});
    }
}
}

LoadingIndicator::LoadingIndicator(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents);
    new MotionVisibility(this, [this] { sync(); });
}

void LoadingIndicator::setColor(const QColor &c)
{
    if (c == m_color)
        return;
    m_color = c;
    emit changed();
    update();
}

void LoadingIndicator::setContainerColor(const QColor &c)
{
    if (c == m_container)
        return;
    m_container = c;
    emit changed();
    update();
}

void LoadingIndicator::setRunning(bool on)
{
    if (on == m_running)
        return;
    m_running = on;
    emit changed();
    sync();
}

void LoadingIndicator::itemChange(ItemChange change, const ItemChangeData &value)
{
    QQuickItem::itemChange(change, value);
    if (change == ItemVisibleHasChanged || change == ItemSceneChange)
        sync();
}

void LoadingIndicator::sync()
{
    const bool go = m_running && motionVisible(this);
    if (go && !m_clock)
        m_clock = new SpringClock(
            [this](double dt) {
                m_t += dt;
                update();
                return false;
            },
            this);
    if (go && m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
    else if (!go && m_clock)
        m_clock->stop();
}

QSGNode *LoadingIndicator::updatePaintNode(QSGNode *old, UpdatePaintNodeData *)
{
    const double size = std::min(width(), height());
    if (size <= 0) {
        delete old;
        return nullptr;
    }
    const bool contained = m_container.alpha() > 0;
    const SpinnerPose pose = spinnerPose(m_t, contained);
    const QPointF c(width() / 2, height() / 2);
    const double s = size * pose.scale;

    if (window()->rendererInterface()->graphicsApi() == QSGRendererInterface::Software) {
        auto *node = static_cast<QSGImageNode *>(old);
        if (!node)
            node = window()->createImageNode();
        const qreal dpr = window()->effectiveDevicePixelRatio();
        QImage img((boundingRect().size() * dpr).toSize(), QImage::Format_ARGB32_Premultiplied);
        img.setDevicePixelRatio(dpr);
        img.fill(Qt::transparent);
        QPainter p(&img);
        p.setRenderHint(QPainter::Antialiasing);
        p.setPen(Qt::NoPen);
        if (contained) {
            p.setBrush(m_container);
            p.drawEllipse(c, size / 2, size / 2);
        }
        p.translate(c);
        p.rotate(pose.rotation);
        p.setBrush(m_color);
        p.drawPath(shapeOrMorph(QLatin1String(pose.from), QLatin1String(pose.to), pose.progress, QRectF(-s / 2, -s / 2, s, s)));
        p.end();
        node->setTexture(window()->createTextureFromImage(img));
        node->setOwnsTexture(true);
        node->setRect(boundingRect());
        return node;
    }

    auto *node = static_cast<QSGGeometryNode *>(old);
    if (!node) {
        node = new QSGGeometryNode;
        auto *g = new QSGGeometry(QSGGeometry::defaultAttributes_ColoredPoint2D(), 0, 0, QSGGeometry::UnsignedIntType);
        g->setDrawingMode(QSGGeometry::DrawTriangles);
        node->setGeometry(g);
        node->setFlag(QSGNode::OwnsGeometry);
        node->setMaterial(new QSGVertexColorMaterial);
        node->setFlag(QSGNode::OwnsMaterial);
    }
    const double fringe = 1.0 / window()->effectiveDevicePixelRatio();
    std::vector<QSGGeometry::ColoredPoint2D> v;
    std::vector<quint32> idx;
    std::vector<QPointF> outline;
    if (contained) {
        const int n = 48;
        outline.reserve(n);
        for (int i = 0; i < n; ++i)
            outline.emplace_back(c.x() + size / 2 * std::cos(kTau * i / n), c.y() + size / 2 * std::sin(kTau * i / n));
        fanWithFringe(v, idx, outline, c, m_container, fringe);
    }
    int count = 0;
    const std::vector<float> cubics = spinnerCubics(pose, &count);
    const double rad = pose.rotation * kTau / 360, cs = std::cos(rad), sn = std::sin(rad);
    auto put = [&](double ux, double uy) {
        const double x = (ux - 0.5) * s, y = (uy - 0.5) * s;
        outline.emplace_back(c.x() + x * cs - y * sn, c.y() + x * sn + y * cs);
    };
    const int steps = std::clamp(int(std::ceil(s * window()->effectiveDevicePixelRatio() / 12)), 2, 8);
    outline.clear();
    for (int i = 0; i < count; ++i) {
        const float *k = cubics.data() + i * 8;
        for (int j = 0; j < steps; ++j) {
            const double t = double(j) / steps, u = 1 - t;
            const double b0 = u * u * u, b1 = 3 * u * u * t, b2 = 3 * u * t * t, b3 = t * t * t;
            put(b0 * k[0] + b1 * k[2] + b2 * k[4] + b3 * k[6], b0 * k[1] + b1 * k[3] + b2 * k[5] + b3 * k[7]);
        }
    }
    outline.erase(std::unique(outline.begin(), outline.end(),
                              [](const QPointF &a, const QPointF &b) { return std::abs(a.x() - b.x()) + std::abs(a.y() - b.y()) < 1e-4; }),
                  outline.end());
    fanWithFringe(v, idx, outline, c, m_color, fringe);

    QSGGeometry *g = node->geometry();
    g->allocate(int(v.size()), int(idx.size()));
    std::copy(v.begin(), v.end(), g->vertexDataAsColoredPoint2D());
    std::copy(idx.begin(), idx.end(), g->indexDataAsUInt());
    node->markDirty(QSGNode::DirtyGeometry);
    return node;
}

WavyProgress::WavyProgress(QQuickItem *parent)
    : QQuickPaintedItem(parent)
    , m_clock(new SpringClock(
          [this](double dt) {
              if (m_moving)
                  m_phase = std::fmod(m_phase + dt * 1.1, 1.0);
              const bool settled = m_shown.step(dt);
              update();
              return settled && !m_moving;
          },
          this))
{
    setAntialiasing(true);
    new MotionVisibility(this, [this] { sync(); });
}

void WavyProgress::setValue(qreal v)
{
    v = std::clamp(v, 0.0, 1.0);
    if (qFuzzyCompare(v + 1, m_value + 1))
        return;
    m_value = v;
    m_shown.target = v;
    if (!m_hasValue || !isVisible() || !window()) {
        m_shown.value = v;
        m_shown.velocity = 0;
    }
    m_hasValue = true;
    emit changed();
    sync();
    update();
}

void WavyProgress::setColor(const QColor &c)
{
    if (c == m_color)
        return;
    m_color = c;
    emit changed();
    update();
}

void WavyProgress::setTrackColor(const QColor &c)
{
    if (c == m_track)
        return;
    m_track = c;
    emit changed();
    update();
}

void WavyProgress::setMoving(bool on)
{
    if (on == m_moving)
        return;
    m_moving = on;
    emit changed();
    sync();
}

void WavyProgress::setAmplitude(qreal a)
{
    if (qFuzzyCompare(a, m_amplitude))
        return;
    m_amplitude = a;
    emit changed();
    update();
}

void WavyProgress::itemChange(ItemChange change, const ItemChangeData &value)
{
    QQuickPaintedItem::itemChange(change, value);
    if (change == ItemVisibleHasChanged || change == ItemSceneChange)
        sync();
}

void WavyProgress::sync()
{
    const bool go = (m_moving || !m_shown.settled()) && motionVisible(this);
    if (go && m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
    else if (!go)
        m_clock->stop();
}

void WavyProgress::paint(QPainter *p)
{
    p->setRenderHint(QPainter::Antialiasing);
    const double thick = 4, gap = 4, wave = 40;
    const double h = height(), w = width(), cy = h / 2;
    const double left = thick / 2, right = w - thick / 2;
    const double value = std::clamp(m_shown.value, 0.0, 1.0);
    const double end = left + (right - left) * value;
    const double amp = std::min(m_amplitude, (h - thick) / 2) * std::clamp(std::min(value / 0.1, (1 - value) / 0.05), 0.0, 1.0);
    if (end - left > 0.5) {
        QPainterPath path;
        path.moveTo(left, cy + amp * std::sin(kTau * (left / wave - m_phase)));
        for (double x = left + 1; x <= end; x += 1)
            path.lineTo(x, cy + amp * std::sin(kTau * (x / wave - m_phase)));
        path.lineTo(end, cy + amp * std::sin(kTau * (end / wave - m_phase)));
        p->setPen(roundPen(m_color, thick));
        p->setBrush(Qt::NoBrush);
        p->drawPath(path);
    }
    const double trackStart = value > 0 ? end + gap + thick : left;
    if (trackStart < right) {
        p->setPen(roundPen(m_track, thick));
        p->drawLine(QPointF(trackStart, cy), QPointF(right, cy));
        p->setPen(Qt::NoPen);
        p->setBrush(m_color);
        p->drawEllipse(QPointF(right, cy), thick / 2, thick / 2);
    }
}

ConnectOrb::ConnectOrb(QQuickItem *parent)
    : QQuickPaintedItem(parent)
    , m_clock(new SpringClock([this](double dt) { return tick(dt); }, this))
{
    setAntialiasing(true);
    m_from = m_to = statusShape(0);
}

qreal ConnectOrb::field() const { return kField; }

void ConnectOrb::componentComplete()
{
    QQuickPaintedItem::componentComplete();
    m_complete = true;
}

void ConnectOrb::setInk(const QColor &c)
{
    if (c == m_ink)
        return;
    m_ink = c;
    emit colorsChanged();
    update();
}

void ConnectOrb::setContainer(const QColor &c)
{
    if (c == m_container)
        return;
    m_container = c;
    emit colorsChanged();
    update();
}

void ConnectOrb::setPressed(bool on)
{
    if (on == m_pressed)
        return;
    m_pressed = on;
    m_press.target = on ? 0.92 : 1.0;
    emit pressedChanged();
    wake();
}

void ConnectOrb::wake()
{
    if (m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
}

void ConnectOrb::startTween(Tween &t, double from, double to, double ms, int easing)
{
    t.from = from;
    t.to = to;
    t.value = from;
    t.elapsed = 0;
    t.duration = ms / 1000.0;
    t.easing = easing;
    t.running = true;
}

void ConnectOrb::show(int s)
{
    if (s != m_shown) {
        m_shown = s;
        emit shownChanged();
    }
    const QString target = statusShape(s);
    if (target != m_to) {
        m_from = m_morph.value < 0.5 ? m_from : m_to;
        m_to = target;
        m_morph.value = 0;
        m_morph.velocity = 0;
        m_morph.target = 1;
    }
    m_thick.target = s == 2 ? 1 : 0;
    wake();
}

void ConnectOrb::setStatus(int s)
{
    if (s == m_status)
        return;
    m_status = s;
    emit statusChanged();
    ++m_generation;
    if (!m_complete) {
        m_shown = s;
        m_from = m_to = statusShape(s);
        m_amp.value = m_amp.target = s == 1 ? 1 : 0;
        m_thick.value = m_thick.target = s == 2 ? 1 : 0;
        if (s == 1) {
            m_since = QDateTime::currentMSecsSinceEpoch();
            wake();
        }
        emit shownChanged();
        update();
        return;
    }
    m_burst.running = false;
    m_burst.value = 0;
    m_shake.running = false;
    m_shake.value = 0;
    m_bounce.value = 1;
    m_bounce.target = 1;
    switch (s) {
    case 1:
        m_since = QDateTime::currentMSecsSinceEpoch();
        show(1);
        m_bounce.value = 0.9;
        m_bounce.stiffness = 400;
        m_bounce.damping = 0.45;
        m_ampTween.running = false;
        m_ampSpring = true;
        m_amp.target = 1;
        break;
    case 2: {
        if (m_amp.value > 0) {
            m_ampTween.running = false;
            m_ampSpring = true;
            m_amp.target = 1;
            const qint64 wait = std::max<qint64>(0, 1200 - (QDateTime::currentMSecsSinceEpoch() - m_since));
            const int gen = m_generation;
            QTimer::singleShot(int(wait), this, [this, gen] {
                if (gen == m_generation)
                    finishOn();
            });
        } else {
            finishOn();
        }
        break;
    }
    default:
        show(s);
        if (s == 3)
            startTween(m_shake, 1, 0, 600, 1);
        m_ampTween.running = false;
        m_ampSpring = true;
        m_amp.target = 0;
        break;
    }
    wake();
}

void ConnectOrb::finishOn()
{
    show(2);
    m_bounce.value = 0.86;
    m_bounce.velocity = 0;
    m_bounce.stiffness = 300;
    m_bounce.damping = 0.32;
    startTween(m_burst, 0, 1, 900, 0);
    m_ampSpring = false;
    startTween(m_ampTween, m_amp.value, 0, 800, 0);
    wake();
}

bool ConnectOrb::tick(double dt)
{
    bool busy = false;
    auto spring = [&](SpringState &s) {
        if (!s.settled() || s.value != s.target) {
            s.step(dt);
            busy = true;
        }
    };
    auto tween = [&](Tween &t) {
        if (!t.running)
            return;
        t.elapsed += dt;
        const double k = t.duration > 0 ? std::min(1.0, t.elapsed / t.duration) : 1.0;
        t.value = t.from + (t.to - t.from) * (t.easing == 0 ? easeDecelerate(k) : easeStandard(k));
        if (k >= 1.0)
            t.running = false;
        busy = true;
    };
    if (m_ampSpring)
        spring(m_amp);
    if (m_ampTween.running) {
        tween(m_ampTween);
        m_amp.value = m_ampTween.value;
        m_amp.velocity = 0;
        m_amp.target = m_ampTween.to;
    }
    spring(m_bounce);
    spring(m_press);
    spring(m_thick);
    spring(m_morph);
    tween(m_burst);
    tween(m_shake);
    if (m_amp.value > 0.0005) {
        m_clockT += dt;
        busy = true;
    }
    update();
    return !busy;
}

void ConnectOrb::paint(QPainter *p)
{
    const Logo &L = logo();
    p->setRenderHint(QPainter::Antialiasing);
    const double w = width(), h = height();
    const double d = std::min(w, h) / kField;
    const double cx = w / 2, cy = h / 2;
    const double amp = std::clamp(m_amp.value, 0.0, 1.0);
    const double t = m_clockT;
    const double full = L.stroke * kLogoH * d;
    const double thin = full * kThin;
    const double width_ = thin + (full - thin) * std::clamp(m_thick.value, 0.0, 1.0);
    const double breath = 1.0 + 0.035 * amp * std::sin(kTau * t / 1.6);
    const double k = m_press.value * m_bounce.value;
    const double shake = m_shake.running ? m_shake.value : 0.0;
    p->translate(cx + shake * 18 * std::sin(shake * 28), cy);
    p->scale(k, k);
    p->translate(-cx, -cy);

    const double s = d * breath;
    p->setPen(Qt::NoPen);
    p->setBrush(m_container);
    const QRectF box(cx - s / 2, cy - s / 2, s, s);
    if (m_morph.value < 1.0 || !m_morph.settled())
        p->drawPath(shapeOrMorph(m_from, m_to, std::max(1e-3, m_morph.value), box));
    else
        p->drawPath(shapePath(m_to, box));

    const double hh = kLogoH * d;
    float px[Logo::N], py[Logo::N];
    for (int i = 0; i < Logo::N; ++i) {
        px[i] = float(cx + L.x[i] * hh);
        py[i] = float(cy + L.y[i] * hh);
    }
    const QPainterPath line = smoothOpen(px, py, Logo::N);
    p->setBrush(Qt::NoBrush);
    if (amp <= 0.0005) {
        p->setPen(roundPen(m_ink, width_));
        p->drawPath(line);
    } else {
        p->setPen(roundPen(withAlpha(m_ink, 1 - 0.78 * amp), width_));
        p->drawPath(line);
        const int loop = L.loop, last = Logo::N - 1;
        const int n = std::max(2, int(loop * 0.18));
        const double head = std::fmod(t / 1.7 * loop, double(loop));
        const double a = 0.011 * d;
        const double phase = 1.4 * t;
        const double fade = n * 0.25, edge = Logo::N * 0.04;
        std::vector<float> wx(static_cast<size_t>(n)), wy(static_cast<size_t>(n));
        QPainterPath active;
        int run = 0;
        auto flush = [&] {
            if (run >= 2)
                active.addPath(smoothOpen(wx.data(), wy.data(), run));
            run = 0;
        };
        for (int k2 = 0; k2 < n; ++k2) {
            const double pp = std::fmod(head - (n - 1 - k2) + loop, double(loop));
            if (pp > last) {
                flush();
                continue;
            }
            const int i0 = int(pp), i1 = std::min(i0 + 1, last);
            const double f = pp - i0;
            const double x = L.lx[size_t(i0)] + (L.lx[size_t(i1)] - L.lx[size_t(i0)]) * f;
            const double y = L.ly[size_t(i0)] + (L.ly[size_t(i1)] - L.ly[size_t(i0)]) * f;
            const double taper = smoothstep01(std::min(k2, n - 1 - k2) / fade) * smoothstep01(std::min(pp, last - pp) / edge);
            const double off = a * taper * std::sin(kTau * (k2 * L.step * hh / (0.075 * d) - phase));
            wx[size_t(run)] = float(cx + x * hh + L.lnx[size_t(i0)] * off);
            wy[size_t(run)] = float(cy + y * hh + L.lny[size_t(i0)] * off);
            ++run;
        }
        flush();
        p->setPen(roundPen(withAlpha(m_ink, amp), thin));
        p->drawPath(active);
    }
    const double b = m_burst.value;
    if (b > 0 && b < 1) {
        QPen ring(withAlpha(m_ink, 0.5 * (1 - b)), full * (1 - b) + 1);
        p->setPen(ring);
        const double r = d / 2 * (1 + (kField - 1) * b);
        p->drawEllipse(QPointF(cx, cy), r, r);
    }
}

LogoMark::LogoMark(QQuickItem *parent)
    : QQuickPaintedItem(parent)
    , m_clock(new SpringClock(
          [this](double dt) {
              m_elapsed += dt;
              m_drawn = easeDecelerate(m_elapsed / 1.1);
              update();
              return m_elapsed >= 1.1;
          },
          this))
{
    setAntialiasing(true);
}

void LogoMark::componentComplete()
{
    QQuickPaintedItem::componentComplete();
    if (m_animated) {
        m_drawn = 0;
        m_elapsed = 0;
        m_clock->start();
    }
}

void LogoMark::setColor(const QColor &c)
{
    if (c == m_color)
        return;
    m_color = c;
    emit changed();
    update();
}

void LogoMark::setAnimated(bool on)
{
    if (on == m_animated)
        return;
    m_animated = on;
    emit changed();
}

void LogoMark::drawMark(QPainter *p, const QPointF &center, double size, const QColor &color, double drawn)
{
    const Logo &L = logo();
    const int n = int(Logo::N * std::clamp(drawn, 0.0, 1.0));
    if (n < 2)
        return;
    const double h = size / (1 + L.stroke);
    float xs[Logo::N], ys[Logo::N];
    for (int i = 0; i < n; ++i) {
        xs[i] = float(center.x() + L.x[i] * h);
        ys[i] = float(center.y() + L.y[i] * h);
    }
    p->setRenderHint(QPainter::Antialiasing);
    p->setBrush(Qt::NoBrush);
    p->setPen(roundPen(color, L.stroke * h));
    p->drawPath(smoothOpen(xs, ys, n));
}

void LogoMark::paint(QPainter *p) { drawMark(p, QPointF(width() / 2, height() / 2), std::min(width(), height()), m_color, m_drawn); }

QrCode::QrCode(QQuickItem *parent)
    : QQuickPaintedItem(parent)
    , m_clock(new SpringClock(
          [this](double dt) {
              m_elapsed += dt;
              m_appear = easeStandard(m_elapsed / 0.7);
              update();
              return m_elapsed >= 0.7;
          },
          this))
{
    setAntialiasing(true);
}

void QrCode::setText(const QString &t)
{
    if (t == m_text)
        return;
    const bool first = m_text.isEmpty();
    m_text = t;
    encode();
    if (!m_stream || first) {
        m_appear = 0;
        m_elapsed = 0;
        m_clock->start();
    }
    emit changed();
    update();
}

void QrCode::setStream(bool on)
{
    if (on == m_stream)
        return;
    m_stream = on;
    encode();
    emit changed();
    update();
}

void QrCode::setInk(const QColor &c)
{
    if (c == m_ink)
        return;
    m_ink = c;
    emit changed();
    update();
}

void QrCode::setBackground(const QColor &c)
{
    if (c == m_bg)
        return;
    m_bg = c;
    emit changed();
    update();
}

void QrCode::encode()
{
    m_n = 0;
    m_bits.clear();
    if (m_text.isEmpty())
        return;
    const QByteArray utf8 = m_text.toUtf8();
    QRcode *code = QRcode_encodeString8bit(utf8.constData(), 0, m_stream ? QR_ECLEVEL_Q : QR_ECLEVEL_H);
    if (!code)
        return;
    m_n = code->width;
    m_bits = QByteArray(reinterpret_cast<const char *>(code->data), m_n * m_n);
    QRcode_free(code);
}

void QrCode::paint(QPainter *p)
{
    if (m_n == 0)
        return;
    p->setRenderHint(QPainter::Antialiasing);
    const int n = m_n;
    const double size = std::min(width(), height());
    const double cell = size / n;
    const int hole = std::max(5, int(n * 0.22) | 1);
    const int h0 = (n - hole) / 2;
    auto on = [&](int x, int y) {
        if (x < 0 || y < 0 || x >= n || y >= n || !(m_bits[y * n + x] & 1))
            return false;
        if ((x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7))
            return false;
        return !(x >= h0 && x < h0 + hole && y >= h0 && y < h0 + hole);
    };
    p->save();
    const double k = 0.94 + 0.06 * m_appear;
    p->translate(size / 2, size / 2);
    p->scale(k, k);
    p->translate(-size / 2, -size / 2);
    p->setOpacity(m_appear);
    p->setPen(Qt::NoPen);
    p->setBrush(m_ink);
    const double r = cell * 0.43;
    for (int y = 0; y < n; ++y)
        for (int x = 0; x < n; ++x) {
            if (!on(x, y))
                continue;
            const double cx = (x + 0.5) * cell, cy = (y + 0.5) * cell;
            p->drawEllipse(QPointF(cx, cy), r, r);
            if (on(x + 1, y))
                p->drawRect(QRectF(cx, cy - r, cell, 2 * r));
            if (on(x, y + 1))
                p->drawRect(QRectF(cx - r, cy, 2 * r, cell));
        }
    const QString finders[] = {QStringLiteral("Cookie9Sided"), QStringLiteral("Clover4Leaf"), QStringLiteral("Sunny")};
    const QPoint corners[] = {{0, 0}, {n - 7, 0}, {0, n - 7}};
    for (int i = 0; i < 3; ++i) {
        const QPointF o(corners[i].x() * cell, corners[i].y() * cell);
        p->setBrush(Qt::NoBrush);
        p->setPen(QPen(m_ink, cell));
        p->drawRoundedRect(QRectF(o + QPointF(cell / 2, cell / 2), QSizeF(cell * 6, cell * 6)), cell * 2.2, cell * 2.2);
        const double side = cell * 3.4;
        p->save();
        p->translate(o + QPointF(cell * 3.5, cell * 3.5));
        p->rotate(m_appear * 45);
        p->setPen(Qt::NoPen);
        p->setBrush(m_ink);
        p->drawPath(shapePath(finders[i], QRectF(-side / 2, -side / 2, side, side)));
        p->restore();
    }
    p->restore();
    const double inner = size * (hole - 1.0) / n * m_appear;
    if (inner > 1) {
        p->setPen(Qt::NoPen);
        p->setBrush(m_ink);
        p->drawPath(shapePath(QStringLiteral("Cookie12Sided"), QRectF(size / 2 - inner / 2, size / 2 - inner / 2, inner, inner)));
        LogoMark::drawMark(p, QPointF(size / 2, size / 2), inner * 0.62, m_bg);
    }
}

ChainWave::ChainWave(QQuickItem *parent)
    : QQuickPaintedItem(parent)
    , m_clock(new SpringClock(
          [this](double dt) {
              m_phase = std::fmod(m_phase + dt / 0.9, 1.0);
              update();
              return false;
          },
          this))
{
    setAntialiasing(true);
    new MotionVisibility(this, [this] { sync(); });
}

void ChainWave::setRunning(bool on)
{
    if (on == m_running)
        return;
    m_running = on;
    sync();
    emit changed();
    update();
}

void ChainWave::sync()
{
    const bool go = m_running && motionVisible(this);
    if (go && m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
    else if (!go)
        m_clock->stop();
}

void ChainWave::setColor(const QColor &c)
{
    if (c == m_color)
        return;
    m_color = c;
    emit changed();
    update();
}

void ChainWave::paint(QPainter *p)
{
    p->setRenderHint(QPainter::Antialiasing);
    const double amp = 3, len = 10, x0 = width() / 2;
    QPainterPath path;
    for (double y = 0; y <= height(); y += 1) {
        const double x = x0 + amp * std::sin((y / len - m_phase) * kTau);
        if (y == 0)
            path.moveTo(x, y);
        else
            path.lineTo(x, y);
    }
    QPen pen(m_color, 2.5);
    pen.setCapStyle(Qt::RoundCap);
    p->setPen(pen);
    p->drawPath(path);
}

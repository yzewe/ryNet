#include "spring.h"

#include "theme.h"

#include <algorithm>
#include <cmath>

bool SpringState::step(double dt)
{
    if (dt <= 0)
        return settled();
    const double w0 = std::sqrt(std::max(1e-6, stiffness));
    const double z = std::max(0.0, damping);
    const double x0 = value - target;
    const double v0 = velocity;
    span = std::max(span, std::abs(x0));
    double x, v;
    if (z < 1.0) {
        const double wd = w0 * std::sqrt(1.0 - z * z);
        const double e = std::exp(-z * w0 * dt);
        const double a = x0;
        const double b = (v0 + z * w0 * x0) / wd;
        const double c = std::cos(wd * dt), s = std::sin(wd * dt);
        x = e * (a * c + b * s);
        v = e * ((b * wd - z * w0 * a) * c - (a * wd + z * w0 * b) * s);
    } else if (z == 1.0) {
        const double e = std::exp(-w0 * dt);
        const double b = v0 + w0 * x0;
        x = e * (x0 + b * dt);
        v = e * (b - w0 * (x0 + b * dt));
    } else {
        const double r = w0 * std::sqrt(z * z - 1.0);
        const double r1 = -z * w0 + r, r2 = -z * w0 - r;
        const double c2 = (v0 - r1 * x0) / (r2 - r1);
        const double c1 = x0 - c2;
        x = c1 * std::exp(r1 * dt) + c2 * std::exp(r2 * dt);
        v = c1 * r1 * std::exp(r1 * dt) + c2 * r2 * std::exp(r2 * dt);
    }
    value = target + x;
    velocity = v;
    span = std::max(span, std::abs(x));
    if (settled()) {
        value = target;
        velocity = 0;
        span = 0;
        return true;
    }
    return false;
}

bool SpringState::settled() const
{
    const double eps = std::clamp(span * 0.002, threshold, std::max(threshold, 0.1));
    return std::abs(value - target) < eps && std::abs(velocity) < eps * 10;
}

void SpringClock::updateCurrentTime(int ms)
{
    const double dt = m_last < 0 ? 0.0 : (ms - m_last) / 1000.0;
    m_last = ms;
    if (m_tick(std::min(dt, 0.064)))
        stop();
}

Spring::Spring(QObject *parent)
    : QObject(parent)
{
}

void Spring::run()
{
    if (m_state.value == m_state.target && m_state.velocity == 0)
        return;
    if (!m_clock) {
        m_clock = new SpringClock([this](double dt) { return tick(dt); }, this);
        connect(m_clock, &QAbstractAnimation::stateChanged, this, &Spring::runningChanged);
    }
    if (m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
}

void Spring::setTarget(const QQmlProperty &p)
{
    m_property = p;
    m_hasTarget = true;
    if (m_initialized)
        write();
    else
        m_state.value = p.read().toDouble();
}

void Spring::setTo(qreal v)
{
    if (!m_initialized) {
        m_initialized = true;
        m_state.target = v;
        snap(v);
        emit toChanged();
        return;
    }
    if (qFuzzyCompare(m_state.target + 1.0, v + 1.0))
        return;
    m_state.target = v;
    emit toChanged();
    if (!m_enabled) {
        snap(v);
        return;
    }
    if (m_hasTarget)
        m_state.value = m_property.read().toDouble();
    run();
}

void Spring::setStiffness(qreal v)
{
    m_state.stiffness = v;
    emit paramsChanged();
}

void Spring::setDamping(qreal v)
{
    m_state.damping = v;
    emit paramsChanged();
}

void Spring::setThreshold(qreal v)
{
    m_state.threshold = v;
    emit paramsChanged();
}

void Spring::setEnabled(bool on)
{
    if (on == m_enabled)
        return;
    m_enabled = on;
    if (!on)
        snap(m_state.target);
    emit paramsChanged();
}

bool Spring::running() const { return m_clock && m_clock->state() == QAbstractAnimation::Running; }

void Spring::snap(qreal v)
{
    if (m_clock)
        m_clock->stop();
    m_initialized = true;
    m_state.span = 0;
    m_state.value = v;
    m_state.target = v;
    m_state.velocity = 0;
    write();
}

void Spring::kick(qreal velocity)
{
    m_state.velocity += velocity;
    run();
}

void Spring::animateFrom(qreal v)
{
    m_state.value = v;
    m_state.velocity = 0;
    write();
    if (!m_enabled) {
        snap(m_state.target);
        return;
    }
    run();
}

void Spring::shift(qreal d)
{
    if (d == 0)
        return;
    m_initialized = true;
    m_state.value += d;
    write();
    if (!m_enabled) {
        snap(m_state.target);
        return;
    }
    run();
}

bool Spring::tick(double dt)
{
    const bool done = m_state.step(dt);
    write();
    return done;
}

void Spring::write()
{
    if (m_hasTarget)
        m_property.write(m_state.value);
}

ColorTo::ColorTo(QObject *parent)
    : QObject(parent)
{
}

void ColorTo::setTarget(const QQmlProperty &p)
{
    m_property = p;
    if (m_set)
        write(m_now);
}

void ColorTo::setTo(const QColor &c)
{
    if (m_set && c == m_to)
        return;
    m_to = c;
    emit toChanged();
    const bool glide = m_set && m_animate && m_duration > 0 && !Theme::instance()->fading() && m_now != c;
    m_set = true;
    if (!glide) {
        if (m_clock)
            m_clock->stop();
        m_now = c;
        write(c);
        return;
    }
    m_from = m_now;
    m_elapsed = 0;
    if (!m_clock)
        m_clock = new SpringClock([this](double dt) { return tick(dt); }, this);
    if (m_clock->state() != QAbstractAnimation::Running)
        m_clock->start();
}

void ColorTo::setAnimate(bool on)
{
    if (on == m_animate)
        return;
    m_animate = on;
    emit paramsChanged();
}

void ColorTo::setDuration(int ms)
{
    if (ms == m_duration)
        return;
    m_duration = ms;
    emit paramsChanged();
}

void ColorTo::setLinear(bool on)
{
    if (on == m_linear)
        return;
    m_linear = on;
    emit paramsChanged();
}

bool ColorTo::tick(double dt)
{
    m_elapsed += dt * 1000;
    const double t = std::min(1.0, m_elapsed / std::max(1, m_duration));
    const double e = m_linear ? t : 1 - std::pow(1 - t, 3);
    const auto mix = [e](float a, float b) { return a + (b - a) * float(e); };
    m_now = QColor::fromRgbF(mix(m_from.redF(), m_to.redF()), mix(m_from.greenF(), m_to.greenF()), mix(m_from.blueF(), m_to.blueF()),
                             mix(m_from.alphaF(), m_to.alphaF()));
    if (t >= 1)
        m_now = m_to;
    write(m_now);
    return t >= 1;
}

void ColorTo::write(const QColor &c)
{
    if (m_property.isValid())
        m_property.write(c);
}

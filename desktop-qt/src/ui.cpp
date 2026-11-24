#include "endpoints.h"
#include "ui.h"

#include "model.h"
#include "spring.h"
#include "theme.h"

#include <QBasicTimer>
#include <QDateTime>
#include <QEasingCurve>
#include <QMouseEvent>
#include <QQuickWindow>
#include <QRegularExpression>
#include <QTimerEvent>
#include <cmath>

namespace {
const auto &kDns = Endpoints::Dns;
const auto &kGoogle = Endpoints::DnsGoogle;

QVariantList presets(bool server)
{
    QVariantList out;
    for (const auto &p : kDns)
        out << QVariant(QVariantList{p.first, p.second});
    if (server)
        out << QVariant(QVariantList{kGoogle.first, kGoogle.second});
    return out;
}

const QEasingCurve &decelerate()
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.05, 0.7), QPointF(0.1, 1), QPointF(1, 1));
        return e;
    }();
    return c;
}

class Shift : public QQuickTransform {
public:
    using QQuickTransform::QQuickTransform;
    void applyTo(QMatrix4x4 *m) const override { m->translate(0, float(y)); }
    void set(double v)
    {
        if (v == y)
            return;
        y = v;
        update();
    }
    double y = 0;
};

class Cascade : public QObject {
public:
    Cascade(QQuickItem *item, double dir, int delay)
        : QObject(item)
        , m_item(item)
        , m_shift(new Shift(this))
    {
        m_rise.stiffness = 420;
        m_rise.damping = 0.78;
        m_rise.value = m_rise.target = 22 * dir;
        m_shift->set(m_rise.value);
        m_shift->appendToItem(item);
        item->setOpacity(0);
        m_clock.start();
        m_wait.start(std::max(1, delay), this);
        m_end.start(delay + 900, this);
    }

protected:
    void timerEvent(QTimerEvent *e) override
    {
        if (e->timerId() == m_wait.timerId()) {
            m_wait.stop();
            m_rise.target = 0;
            m_fadeAt = m_clock.elapsed();
            if (!m_driver)
                m_driver = new SpringClock([this](double dt) { return tick(dt); }, this);
            m_driver->start();
        } else if (e->timerId() == m_end.timerId()) {
            deleteLater();
        }
    }

private:
    bool tick(double dt)
    {
        const bool rest = m_rise.step(dt);
        m_shift->set(m_rise.value);
        const double t = std::min(1.0, (m_clock.elapsed() - m_fadeAt) / 260.0);
        if (m_item)
            m_item->setOpacity(decelerate().valueForProgress(t));
        return rest && t >= 1;
    }

    QPointer<QQuickItem> m_item;
    Shift *m_shift;
    SpringState m_rise;
    QElapsedTimer m_clock;
    qint64 m_fadeAt = 0;
    QBasicTimer m_wait, m_end;
    SpringClock *m_driver = nullptr;
};
}

Ui *Ui::instance()
{
    static Ui *u = new Ui;
    return u;
}

Ui *Ui::create(QQmlEngine *, QJSEngine *)
{
    QJSEngine::setObjectOwnership(instance(), QJSEngine::CppOwnership);
    return instance();
}

Ui::Ui(QObject *parent)
    : QObject(parent)
{
}

void Ui::setOverlay(QQuickItem *o)
{
    if (o == m_overlay)
        return;
    m_overlay = o;
    if (o && o->window())
        o->window()->installEventFilter(this);
    emit overlayChanged();
}

bool Ui::eventFilter(QObject *o, QEvent *e)
{
    if (e->type() == QEvent::MouseButtonPress && m_overlay && o == m_overlay->window()) {
        m_pressAt = m_overlay->mapFromScene(static_cast<QMouseEvent *>(e)->scenePosition());
        m_pressed.start();
    }
    return QObject::eventFilter(o, e);
}

void Ui::setWidthClass(int c)
{
    if (c == m_widthClass)
        return;
    m_widthClass = c;
    emit widthClassChanged();
}

void Ui::setSnackHeight(qreal h)
{
    if (h == m_snackHeight)
        return;
    m_snackHeight = h;
    emit snackHeightChanged();
}

void Ui::setOuter(qreal v)
{
    if (v == m_outer)
        return;
    m_outer = v;
    emit sectionChanged();
}

void Ui::setInner(qreal v)
{
    if (v == m_inner)
        return;
    m_inner = v;
    emit sectionChanged();
}

void Ui::setGap(qreal v)
{
    if (v == m_gap)
        return;
    m_gap = v;
    emit sectionChanged();
}

void Ui::setSplit(bool on)
{
    if (on == m_split)
        return;
    m_split = on;
    emit splitChanged();
}

void Ui::setFontFamily(const QString &f)
{
    if (f == m_fontFamily)
        return;
    m_fontFamily = f;
    emit fontFamilyChanged();
}

void Ui::setTipItem(QQuickItem *i)
{
    if (i == m_tipItem)
        return;
    m_tipItem = i;
    emit tipItemChanged();
}

void Ui::setTipText(const QString &t)
{
    if (t == m_tipText)
        return;
    m_tipText = t;
    emit tipTextChanged();
}

QVariantList Ui::dnsPresets() const { return presets(false); }

QVariantList Ui::serverDnsPresets() const { return presets(true); }

QString Ui::geoPage(const QString &kind, const QString &name) const
{
    return Endpoints::GeoPages + u'/' + kind + u'/' + name.toLower() + QStringLiteral(".list");
}

QString Ui::tr(const QString &ru, const QString &en) const { return Lang::ru() ? ru : en; }

void Ui::showTip(QQuickItem *item, const QString &text)
{
    setTipItem(item);
    setTipText(text);
}

void Ui::hideTip(QQuickItem *item)
{
    if (m_tipItem == item)
        setTipText(QString());
}

QString Ui::flagCode(const QString &emoji) const
{
    const QList<uint> cps = emoji.left(4).toUcs4();
    if (cps.size() < 2)
        return {};
    const uint a = cps[0], b = cps[1];
    if (a < 0x1F1E6 || a > 0x1F1FF || b < 0x1F1E6 || b > 0x1F1FF)
        return {};
    return QString(QChar(char16_t(u'a' + (a - 0x1F1E6)))) + QChar(char16_t(u'a' + (b - 0x1F1E6)));
}

QStringList Ui::splitFlag(const QString &name) const
{
    const QString p = name.trimmed();
    if (p.size() >= 4 && !flagCode(p.left(4)).isEmpty())
        return {p.left(4), p.mid(4).trimmed()};
    return {QString(), p};
}

QString Ui::dnsLabel(const QString &value) const
{
    if (value.isEmpty())
        return QStringLiteral("DNS");
    for (const auto &p : kDns)
        if (p.second == value)
            return p.first;
    if (kGoogle.second == value)
        return kGoogle.first;
    return tr(QStringLiteral("Свой DNS"), QStringLiteral("Custom DNS"));
}

QString Ui::dnsChoice(const QString &value) const
{
    const QString v = value.trimmed();
    if (v.isEmpty())
        return QStringLiteral("auto");
    for (const auto &p : kDns)
        if (p.second == v)
            return p.second;
    return QStringLiteral("custom");
}

bool Ui::serverDnsPreset(const QString &value) const
{
    return kGoogle.second == value || std::any_of(kDns.begin(), kDns.end(), [&](const auto &p) { return p.second == value; });
}

QStringList Ui::dnsOptions() const
{
    QStringList out{QStringLiteral("auto")};
    for (const auto &p : kDns)
        out << p.second;
    return out << QStringLiteral("custom");
}

QStringList Ui::dnsLabels() const
{
    QStringList out{tr(QStringLiteral("Авто"), QStringLiteral("Auto"))};
    for (const auto &p : kDns)
        out << p.first;
    return out << tr(QStringLiteral("Своё"), QStringLiteral("Custom"));
}

QString Ui::dnsSummary(const QString &value) const
{
    const QString choice = dnsChoice(value);
    if (choice == u"custom") {
        static const QRegularExpression by(QStringLiteral("[\\s,]+"));
        return value.trimmed().split(by, Qt::SkipEmptyParts).join(QStringLiteral(", "));
    }
    return dnsLabels().value(dnsOptions().indexOf(choice));
}

QColor Ui::pingColor(int ms) const
{
    const Theme *t = Theme::instance();
    if (ms < 0)
        return t->error();
    return ms < 300 ? t->primary() : ms < 800 ? t->warning() : t->error();
}

QVariantList Ui::setPreview(const QVariantMap &set) const
{
    QVariantList out;
    const std::pair<const char *, const char *> kinds[] = {{"domains", "language"}, {"procs", "apps"}, {"geosite", "list"}, {"ips", "conversion_path"}};
    for (const auto &[key, icon] : kinds) {
        const QStringList values = set.value(QLatin1String(key)).toStringList();
        for (const QString &v : values.mid(0, QLatin1String(key) == QLatin1String("domains") ? 10 : 6))
            out << QVariant(QVariantList{QLatin1String(icon), v});
    }
    return out;
}

QVariantMap Ui::errorReason(const QString &message) const
{
    const QString m = message.trimmed();
    const QString update = tr(QStringLiteral("Обновите подписку или выберите другой сервер"), QStringLiteral("Update the subscription or choose another server"));
    const QString other = tr(QStringLiteral("Выберите другой сервер"), QStringLiteral("Choose another server"));
    const QString net = tr(QStringLiteral("Проверьте интернет или выберите другой сервер"), QStringLiteral("Check the internet or choose another server"));
    const QString failed = tr(QStringLiteral("Не удалось подключиться"), QStringLiteral("Could not connect"));
    const auto reason = [&](const QString &title, const QString &hint, bool details = true) {
        return QVariantMap{{"title", title}, {"hint", hint}, {"details", details && !m.isEmpty() && m != title ? m : QString()}};
    };
    const auto has = [&](std::initializer_list<const char *> parts) {
        return std::any_of(parts.begin(), parts.end(), [&](const char *p) { return m.contains(QString::fromUtf8(p), Qt::CaseInsensitive); });
    };
    const auto t = [this](const char *ru, const char *en) { return tr(QString::fromUtf8(ru), QString::fromUtf8(en)); };
    if (m.isEmpty())
        return reason(failed, net, false);
    if (has({"Не выбран сервер", "No server selected"}))
        return reason(t("Сервер не выбран", "No server selected"), t("Выберите сервер в списке", "Choose a server in the list"), false);
    if (has({"ключ REALITY", "REALITY key"}))
        return reason(t("Сервер отклонил ключ", "The server rejected the key"), update);
    if (has({"Неверный пароль", "неверный UUID", "Wrong password", "wrong UUID"}))
        return reason(t("Сервер не принял пароль", "The server rejected the password"), update);
    if (has({"Сертификат", "certificate"}))
        return reason(t("Сертификат не прошёл проверку", "Certificate check failed"), update);
    if (has({"отказал в TLS", "не по TLS", "ошибкой HTTP", "HTTP/2", "не так, как ждали", "rejected TLS", "answer with TLS", "HTTP error", "Unexpected server reply"}))
        return reason(t("Сервер не принял настройки", "The server rejected the settings"), update);
    if (has({"Разрыв TLS", "блокировк", "TLS dropped", "blocking"}))
        return reason(t("Похоже на блокировку", "Looks like blocking"), t("Попробуйте другой сервер или протокол", "Try another server or protocol"));
    if (has({"Таймаут", "Timeout"}))
        return reason(t("Сервер не отвечает", "The server does not respond"), net);
    if (has({"отклонил соединение", "refused the connection"}))
        return reason(t("Сервер не принимает соединения", "The server refuses connections"), other);
    if (has({"закрыл соединение", "оборвал поток", "Соединение прервано", "closed the connection", "dropped the stream", "Connection aborted"}))
        return reason(t("Сервер оборвал соединение", "The server dropped the connection"), update);
    if (has({"Нет сети", "Нет доступа к сети", "No network"}))
        return reason(t("Нет сети", "No network"), t("Проверьте подключение к интернету", "Check your internet connection"));
    if (has({"недоступен", "unreachable", "not available"}))
        return reason(t("Нет связи с сервером", "Cannot reach the server"), net);
    if (has({"(DNS)"}))
        return reason(t("Адрес сервера не найден", "Server address not found"), t("Проверьте интернет или обновите подписку", "Check the internet or update the subscription"));
    if (has({"Неправильная конфигурация", "Invalid configuration"}))
        return reason(t("Сервер настроен неверно", "The server is misconfigured"), update);
    if (has({"Не поддерживается", "not supported"}))
        return reason(t("Сервер не поддерживается", "The server is not supported"), other);
    if (has({"Ядро остановилось", "Core stopped"}))
        return reason(t("Подключение прервалось", "The connection was interrupted"), t("Попробуйте ещё раз", "Try again"));
    QString head = m.section(QStringLiteral(": "), 0, 0);
    for (const QString &p : {QStringLiteral("Ошибка"), QStringLiteral("Error")})
        if (head.startsWith(p))
            head = head.mid(p.size());
    head = head.trimmed();
    if (head.isEmpty())
        head = failed;
    head[0] = head[0].toUpper();
    return reason(head, net);
}

QString Ui::shortLink(const QString &link) const
{
    const qsizetype at = link.indexOf(QStringLiteral("://"));
    if (at < 0)
        return link;
    QString rest = link.mid(at + 3).section(QLatin1Char('#'), 0, 0).section(QLatin1Char('?'), 0, 0);
    rest = rest.section(QLatin1Char('@'), -1).section(QLatin1Char('/'), 0, 0);
    return link.left(at) + QStringLiteral("://") + rest;
}

QString Ui::joined(const QStringList &parts, const QString &sep) const
{
    QStringList out;
    for (const QString &p : parts)
        if (!p.isEmpty())
            out << p;
    return out.join(sep);
}

QStringList Ui::suggest(const QStringList &list, const QString &text, int limit) const
{
    const QString last = text.section(QLatin1Char('\n'), -1).trimmed().toLower();
    QStringList out;
    for (const QString &p : list) {
        if (out.size() >= limit)
            break;
        if (last.isEmpty() || p.toLower().contains(last))
            out << p;
    }
    return out;
}

QVariantList Ui::usable(const QVariantList &servers) const
{
    QVariantList out;
    for (const QVariant &v : servers) {
        const QVariantMap m = v.toMap();
        if (m.value(QStringLiteral("supported")).toBool() && !m.value(QStringLiteral("placeholder")).toBool())
            out << v;
    }
    return out;
}

int Ui::indexOfKey(const QVariantList &list, const QString &field, const QVariant &value) const
{
    for (int i = 0; i < list.size(); ++i)
        if (list[i].toMap().value(field) == value)
            return i;
    return -1;
}

QStringList Ui::pluck(const QVariantList &list, const QString &field) const
{
    QStringList out;
    for (const QVariant &v : list)
        out << v.toMap().value(field).toString();
    return out;
}

QStringList Ui::toggled(const QStringList &list, const QString &item) const
{
    QStringList out = list;
    if (!out.removeOne(item))
        out << item;
    return out;
}

int Ui::countEntries(const QVariantMap &lists) const
{
    int n = 0;
    for (const QVariant &v : lists)
        n += int(v.toList().size());
    return n;
}

QString Ui::hostList(const QString &text) const
{
    static const QRegularExpression gaps(QStringLiteral("[\\s,;]+"));
    return text.split(gaps, Qt::SkipEmptyParts).join(QStringLiteral(", "));
}

int Ui::nearest(const QVariantList &steps, double value) const
{
    int best = 0;
    for (int i = 1; i < steps.size(); ++i)
        if (std::abs(steps[i].toDouble() - value) < std::abs(steps[best].toDouble() - value))
            best = i;
    return best;
}

void Ui::groupEdges(QQuickItem *column, bool first, bool last, bool folds) const
{
    if (!column)
        return;
    QList<QQuickItem *> shown;
    for (QQuickItem *c : column->childItems()) {
        if (!c->property("groupFirst").isValid())
            continue;
        const bool fold = c->property("fold").toBool();
        const bool open = fold && c->property("open").toBool();
        if (folds && fold ? open : c->isVisible())
            shown << c;
    }
    for (int i = 0; i < shown.size(); ++i) {
        shown[i]->setProperty("groupFirst", first && i == 0);
        shown[i]->setProperty("groupLast", last && i == shown.size() - 1);
    }
}

QStringList Ui::without(const QStringList &list, const QString &value) const
{
    QStringList out = list;
    out.removeAll(value);
    return out;
}

QStringList Ui::merged(const QStringList &list, const QStringList &more) const
{
    QStringList out = list;
    for (const QString &v : more)
        if (!out.contains(v))
            out << v;
    return out;
}

bool Ui::within(QQuickItem *item, QQuickItem *root) const
{
    for (QQuickItem *i = item; i; i = i->parentItem())
        if (i == root)
            return true;
    return false;
}

void Ui::moveFocus(QQuickItem *root, QQuickItem *from, int by) const
{
    if (!root)
        return;
    QList<QQuickItem *> stops;
    const std::function<void(QQuickItem *)> walk = [&](QQuickItem *item) {
        for (QQuickItem *c : item->childItems()) {
            if (!c->isVisible() || !c->isEnabled())
                continue;
            const QVariant readOnly = c->property("readOnly");
            if (c->activeFocusOnTab() || (readOnly.isValid() && !readOnly.toBool() && c->property("cursorPosition").isValid()))
                stops << c;
            walk(c);
        }
    };
    walk(root);
    if (stops.isEmpty()) {
        root->forceActiveFocus();
        return;
    }
    const qsizetype at = stops.indexOf(from);
    const qsizetype n = stops.size();
    const qsizetype next = at < 0 ? (by > 0 ? 0 : n - 1) : ((at + by) % n + n) % n;
    stops[next]->forceActiveFocus(by > 0 ? Qt::TabFocusReason : Qt::BacktabFocusReason);
}

QQuickItem *Ui::takeFocus(QQuickItem *scope, QQuickItem *returnTo) const
{
    if (!scope || !scope->window())
        return returnTo;
    QQuickItem *f = scope->window()->activeFocusItem();
    if (within(f, scope))
        return returnTo;
    scope->forceActiveFocus();
    return f;
}

void Ui::giveFocusBack(QQuickItem *layer, QQuickItem *returnTo) const
{
    QQuickWindow *w = layer ? layer->window() : nullptr;
    QQuickItem *f = w ? w->activeFocusItem() : nullptr;
    if (f && !within(f, layer))
        return;
    if (returnTo && w && returnTo != w->contentItem() && returnTo->isVisible() && returnTo->isEnabled() && within(returnTo, w->contentItem()))
        returnTo->forceActiveFocus();
    else if (m_overlay)
        m_overlay->forceActiveFocus();
}

qreal Ui::menuItems(QQuickItem *column, QObject *menu) const
{
    qreal widest = 0;
    if (!column)
        return widest;
    for (QQuickItem *c : column->childItems()) {
        if (c->property("menu").isValid())
            c->setProperty("menu", QVariant::fromValue(menu));
        if (c->isVisible())
            widest = std::max(widest, c->implicitWidth());
    }
    return widest;
}

void Ui::menuSweep(QQuickItem *column, qreal edge, bool upward) const
{
    if (!column)
        return;
    for (QQuickItem *c : column->childItems()) {
        if (!c->isVisible() || c->height() <= 0)
            continue;
        const qreal top = column->y() + c->y();
        const qreal v = std::clamp((upward ? top + c->height() - edge : edge - top) / (c->height() * 0.75), 0.0, 1.0);
        if (c->property("enter").isValid())
            c->setProperty("enter", v);
        else
            c->setOpacity(v);
    }
}

QPointF Ui::growFrom(qreal w, qreal h) const
{
    if (!m_pressed.isValid() || m_pressed.elapsed() >= 700)
        return {};
    return {std::clamp(m_pressAt.x() - w / 2, -360.0, 360.0), std::clamp(m_pressAt.y() - h / 2, -280.0, 280.0)};
}

void Ui::cascadeIn(QQuickItem *root, int dir)
{
    if (!root)
        return;
    QList<QQuickItem *> units;
    const std::function<void(QQuickItem *)> walk = [&](QQuickItem *item) {
        for (QQuickItem *c : item->childItems()) {
            if (!c->isVisible())
                continue;
            if (c->property("groupFirst").isValid() || c->property("cascadeUnit").toBool())
                units << c;
            else
                walk(c);
        }
    };
    walk(root);
    for (int i = 0; i < units.size(); ++i)
        cascade(units[i], dir, 20 + std::min(i, 12) * 26);
}

void Ui::cascade(QQuickItem *item, qreal dir, int delay)
{
    if (item)
        new Cascade(item, dir, delay);
}

void Ui::cascadeAt(QQuickItem *item, qreal dir, qreal y, double since)
{
    const double late = double(QDateTime::currentMSecsSinceEpoch()) - since;
    cascade(item, dir, int(std::max(0.0, 60 + std::min(12.0, std::max(0.0, y / 64)) * 26 - late)));
}

void Ui::cascadeDown(QQuickItem *root, qreal dir, double since)
{
    if (!root)
        return;
    const std::function<void(QQuickItem *)> walk = [&](QQuickItem *item) {
        for (QQuickItem *c : item->childItems()) {
            if (!c->isVisible() || c->opacity() == 0)
                continue;
            if (c->property("groupFirst").isValid() || c->property("cascadeUnit").toBool())
                cascadeAt(c, dir, c->mapToItem(root, QPointF()).y(), since);
            else
                walk(c);
        }
    };
    walk(root);
}

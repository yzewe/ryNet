#include "logmodel.h"

#include "store.h"

#include <QDateTime>
#include <QFile>
#include <QRegularExpression>

namespace {
enum Roles { Time = Qt::UserRole, Level, Tag, Text, Fresh };

QStringList tail(const QString &file)
{
    QFile f(file);
    if (!f.open(QIODevice::ReadOnly))
        return {};
    const qint64 size = f.size();
    const qint64 from = std::max<qint64>(0, size - 256 * 1024);
    f.seek(from);
    QString text = QString::fromUtf8(f.readAll());
    static const QRegularExpression ansi(QStringLiteral("\x1B\\[[0-9;]*m"));
    text.remove(ansi);
    QStringList lines = text.split(u'\n');
    if (from > 0 && !lines.isEmpty())
        lines.removeFirst();
    QStringList out;
    for (const QString &l : std::as_const(lines))
        if (!l.trimmed().isEmpty())
            out << l;
    if (out.size() > 2000)
        out = out.mid(out.size() - 2000);
    return out;
}
}

LogModel::LogModel(QObject *parent)
    : QAbstractListModel(parent)
{
    m_timer.setInterval(1000);
    connect(&m_timer, &QTimer::timeout, this, &LogModel::poll);
}

LogModel::Line LogModel::parse(const QString &l)
{
    static const QRegularExpression re(QStringLiteral("^(\\d{4}-\\d\\d-\\d\\dT\\d\\d:\\d\\d:\\d\\d\\S*)\\s+(ERROR|WARN|INFO|DEBUG|TRACE)\\s+(?:([\\w:]+):\\s+)?(.*)$"));
    const auto m = re.match(l);
    Line line;
    if (!m.hasMatch()) {
        line.text = l;
        line.raw = l;
        return line;
    }
    const QDateTime at = QDateTime::fromString(m.captured(1), Qt::ISODateWithMs);
    line.time = at.isValid() ? at.toLocalTime().toString(QStringLiteral("HH:mm:ss")) : m.captured(1).mid(11, 8);
    line.level = m.captured(2);
    const QString tag = m.captured(3);
    const int sep = tag.lastIndexOf(QStringLiteral("::"));
    line.tag = sep >= 0 ? tag.mid(sep + 2) : tag;
    line.text = m.captured(4);
    QStringList parts{line.time, line.level};
    if (!line.tag.isEmpty())
        parts << line.tag + u':';
    parts << line.text;
    line.raw = parts.join(u' ');
    return line;
}

void LogModel::setLevel(const QString &l)
{
    if (l == m_level)
        return;
    m_level = l;
    emit filterChanged();
    refilter();
}

void LogModel::setQuery(const QString &q)
{
    if (q == m_query)
        return;
    m_query = q;
    emit filterChanged();
    refilter();
}

void LogModel::setPaused(bool on)
{
    if (on == m_paused)
        return;
    m_paused = on;
    emit pausedChanged();
    if (!on && m_active)
        poll();
}

void LogModel::setActive(bool on)
{
    if (on == m_active)
        return;
    m_active = on;
    emit activeChanged();
    if (on) {
        poll();
        m_timer.start();
    } else {
        m_timer.stop();
    }
}

bool LogModel::matches(const Line &l) const
{
    if (m_level == u"info" && (l.level == u"WARN" || l.level == u"ERROR"))
        return false;
    if (m_level == u"warn" && l.level != u"WARN")
        return false;
    if (m_level == u"error" && l.level != u"ERROR")
        return false;
    return m_query.trimmed().isEmpty() || l.raw.contains(m_query.trimmed(), Qt::CaseInsensitive);
}

void LogModel::refilter()
{
    beginResetModel();
    m_shown.clear();
    for (int i = 0; i < m_parsed.size(); ++i) {
        m_parsed[i].fresh = false;
        if (matches(m_parsed[i]))
            m_shown << i;
    }
    endResetModel();
    emit countsChanged();
}

void LogModel::reload()
{
    m_raw.clear();
    poll();
}

void LogModel::poll()
{
    if (m_paused)
        return;
    const QStringList next = tail(Store::path(QStringLiteral("core.log")));
    if (next == m_raw)
        return;
    int keep = -1;
    if (!m_raw.isEmpty()) {
        const QString last = m_raw.last();
        for (int p = int(next.size()) - 1; p >= 0 && keep < 0; --p) {
            if (next[p] != last)
                continue;
            bool same = true;
            for (int k = 1; k <= std::min({5, p, int(m_raw.size()) - 1}); ++k)
                if (next[p - k] != m_raw[m_raw.size() - 1 - k]) {
                    same = false;
                    break;
                }
            if (same)
                keep = p;
        }
    }
    auto count = [this] {
        int w = 0, e = 0;
        for (const Line &l : std::as_const(m_parsed)) {
            if (l.level == u"WARN")
                ++w;
            else if (l.level == u"ERROR")
                ++e;
        }
        m_warnings = w;
        m_errors = e;
    };
    if (keep < 0) {
        m_raw = next;
        beginResetModel();
        m_parsed.clear();
        m_shown.clear();
        for (const QString &l : next) {
            m_parsed << parse(l);
            if (matches(m_parsed.last()))
                m_shown << int(m_parsed.size()) - 1;
        }
        endResetModel();
        count();
        emit countsChanged();
        return;
    }
    const int dropped = int(m_raw.size()) - (keep + 1);
    if (dropped > 0) {
        int rows = 0;
        while (rows < m_shown.size() && m_shown[rows] < dropped)
            ++rows;
        if (rows > 0) {
            beginRemoveRows({}, 0, rows - 1);
            m_shown.remove(0, rows);
            endRemoveRows();
        }
        m_parsed.remove(0, dropped);
        for (int &i : m_shown)
            i -= dropped;
    }
    for (Line &l : m_parsed)
        l.fresh = false;
    QList<int> added;
    for (int i = keep + 1; i < next.size(); ++i) {
        Line l = parse(next[i]);
        l.fresh = true;
        m_parsed << l;
        if (matches(l))
            added << int(m_parsed.size()) - 1;
    }
    if (!added.isEmpty()) {
        beginInsertRows({}, int(m_shown.size()), int(m_shown.size() + added.size()) - 1);
        m_shown << added;
        endInsertRows();
    }
    m_raw = next;
    count();
    emit countsChanged();
}

int LogModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_shown.size()); }

QVariant LogModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_shown.size())
        return {};
    const Line &l = m_parsed.at(m_shown.at(index.row()));
    switch (role) {
    case Time:
        return l.time;
    case Level:
        return l.level;
    case Tag:
        return l.tag;
    case Text:
        return l.text;
    case Fresh:
        return l.fresh;
    }
    return {};
}

QHash<int, QByteArray> LogModel::roleNames() const
{
    return {{Time, "time"}, {Level, "level"}, {Tag, "tag"}, {Text, "text"}, {Fresh, "fresh"}};
}

QString LogModel::text() const
{
    QStringList out;
    for (int i : m_shown)
        out << m_parsed.at(i).raw;
    return out.join(u'\n');
}

void LogModel::clear()
{
    QFile f(Store::path(QStringLiteral("core.log")));
    if (f.open(QIODevice::WriteOnly | QIODevice::Truncate))
        f.close();
    m_raw.clear();
    beginResetModel();
    m_parsed.clear();
    m_shown.clear();
    endResetModel();
    m_warnings = m_errors = 0;
    emit countsChanged();
}

#include "grid.h"

#include "app.h"

#include <algorithm>
#include <cmath>

#include <QSet>

KeyedModel::KeyedModel(const QStringList &roles, QObject *parent)
    : QAbstractListModel(parent)
    , m_roles(roles)
{
}

int KeyedModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_items.size()); }

QVariant KeyedModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_items.size())
        return {};
    const int r = role - Qt::UserRole;
    if (r < 0 || r >= m_roles.size())
        return {};
    return m_items.at(index.row()).value(m_roles.at(r));
}

QHash<int, QByteArray> KeyedModel::roleNames() const
{
    QHash<int, QByteArray> out;
    for (int i = 0; i < m_roles.size(); ++i)
        out.insert(Qt::UserRole + i, m_roles.at(i).toUtf8());
    return out;
}

QVariantMap KeyedModel::get(int row) const { return row >= 0 && row < m_items.size() ? m_items.at(row) : QVariantMap(); }

int KeyedModel::indexOf(const QString &key) const
{
    for (int i = 0; i < m_items.size(); ++i)
        if (m_items.at(i).value(QStringLiteral("key")).toString() == key)
            return i;
    return -1;
}

int KeyedModel::roleIndex(const QString &name) const
{
    const int index = m_roles.indexOf(name);
    return index < 0 ? -1 : Qt::UserRole + index;
}

void KeyedModel::setItems(const QList<QVariantMap> &items)
{
    const int before = int(m_items.size());
    QSet<QString> wanted;
    for (const QVariantMap &m : items)
        wanted.insert(m.value(QStringLiteral("key")).toString());
    for (int i = int(m_items.size()) - 1; i >= 0; --i) {
        if (wanted.contains(m_items.at(i).value(QStringLiteral("key")).toString()))
            continue;
        int start = i;
        while (start > 0 && !wanted.contains(m_items.at(start - 1).value(QStringLiteral("key")).toString()))
            --start;
        beginRemoveRows({}, start, i);
        m_items.remove(start, i - start + 1);
        endRemoveRows();
        i = start;
    }
    if (m_items.isEmpty() && !items.isEmpty()) {
        beginInsertRows({}, 0, items.size() - 1);
        m_items = items;
        endInsertRows();
        if (before != m_items.size())
            emit countChanged();
        return;
    }
    QStringList keys;
    QSet<QString> existing;
    bool knowExisting = false;
    keys.reserve(m_items.size());
    for (const QVariantMap &item : std::as_const(m_items)) {
        const QString key = item.value(QStringLiteral("key")).toString();
        keys << key;
    }
    for (int i = 0; i < items.size(); ++i) {
        const QString key = items.at(i).value(QStringLiteral("key")).toString();
        const int at = keys.indexOf(key, i);
        if (at < 0) {
            if (!knowExisting) {
                existing = QSet<QString>(keys.begin(), keys.end());
                knowExisting = true;
            }
            int end = i + 1;
            while (end < items.size() && !existing.contains(items.at(end).value(QStringLiteral("key")).toString()))
                ++end;
            beginInsertRows({}, i, end - 1);
            for (int j = i; j < end; ++j) {
                m_items.insert(j, items.at(j));
                keys.insert(j, items.at(j).value(QStringLiteral("key")).toString());
            }
            endInsertRows();
            i = end - 1;
            continue;
        }
        if (at != i) {
            beginMoveRows({}, at, at, {}, i);
            m_items.move(at, i);
            keys.move(at, i);
            endMoveRows();
        }
        if (m_items.at(i) != items.at(i)) {
            QList<int> roles;
            for (int r = 0; r < m_roles.size(); ++r)
                if (m_items.at(i).value(m_roles.at(r)) != items.at(i).value(m_roles.at(r)))
                    roles << Qt::UserRole + r;
            m_items[i] = items.at(i);
            emit dataChanged(index(i), index(i), roles);
        }
    }
    if (before != m_items.size())
        emit countChanged();
}

namespace {
const QStringList kRoles = {"key", "kind", "title", "note", "count", "remarks", "name", "flag", "line", "lineError", "supported",
                            "selected", "focused", "waiting", "ping", "pingFail", "members", "wide", "current", "text"};
}

QVariantMap ServerGrid::Row::map() const
{
    if (kind != u"tile")
        return {{"key", key}, {"kind", kind}, {"title", title}, {"note", note}, {"count", count}, {"text", text}};
    return {{"key", key},           {"kind", kind},         {"remarks", remarks},     {"name", name},
            {"flag", flag},         {"line", line},         {"lineError", lineError}, {"supported", supported},
            {"selected", selected}, {"focused", focused},   {"waiting", waiting},     {"ping", ping},
            {"pingFail", pingFail}, {"members", members},   {"wide", wide},           {"current", current}};
}

ServerGrid::ServerGrid(QObject *parent)
    : QObject(parent)
    , m_visibleItems(new KeyedModel(kRoles, this))
{
    m_debounce.setSingleShot(true);
    m_debounce.setInterval(0);
    connect(&m_debounce, &QTimer::timeout, this, &ServerGrid::rebuild);
    m_clock.start();
    m_later.setSingleShot(true);
    m_later.setInterval(0);
    connect(&m_later, &QTimer::timeout, this, &ServerGrid::relayout);
    m_settle.setSingleShot(true);
    connect(&m_settle, &QTimer::timeout, this, &ServerGrid::relayout);
    m_release.setSingleShot(true);
    m_release.setInterval(900);
    connect(&m_release, &QTimer::timeout, this, [this] {
        m_retained.clear();
        updateVisible();
    });
    m_unsnap.setSingleShot(true);
    m_unsnap.setInterval(0);
    connect(&m_unsnap, &QTimer::timeout, this, [this] { m_snapping = false; });
    App *app = App::instance();
    connect(app, &App::serversChanged, this, &ServerGrid::schedule);
    connect(app, &App::pingsChanged, this, &ServerGrid::schedule);
    connect(app, &App::settingsChanged, this, &ServerGrid::schedule);
    connect(app, &App::entryChanged, this, &ServerGrid::schedule);
    connect(app, &App::subsChanged, this, &ServerGrid::schedule);
    rebuild();
}

void ServerGrid::showRows(const QStringList &keys)
{
    QList<QVariantMap> rows;
    rows.reserve(keys.size());
    for (const QString &key : keys) {
        const int at = m_rowOf.value(key, -1);
        if (at >= 0)
            rows << m_rows.at(at).map();
    }
    m_visibleItems->setItems(rows);
}

void ServerGrid::setQuery(const QString &q)
{
    if (q == m_query)
        return;
    m_query = q;
    emit queryChanged();
    schedule();
}

void ServerGrid::setPinSelected(bool on)
{
    if (on == m_pinSelected)
        return;
    m_pinSelected = on;
    emit pinSelectedChanged();
    schedule();
}

void ServerGrid::setEntry(bool on)
{
    if (on == m_entry)
        return;
    m_entry = on;
    emit entryChanged();
    schedule();
}

void ServerGrid::setFocused(const QString &f)
{
    if (f == m_focused)
        return;
    m_focused = f;
    emit focusedChanged();
    schedule();
}

void ServerGrid::schedule() { m_debounce.start(); }

QStringList ServerGrid::pingTargets()
{
    rebuild();
    QStringList out;
    for (const Row &row : std::as_const(m_rows))
        if (row.kind == u"tile" && row.supported)
            out << row.remarks;
    out.removeDuplicates();
    return out;
}

void ServerGrid::rebuild()
{
    App *app = App::instance();
    const Settings &s = app->settingsRef();
    const QList<Server> &base = m_entry ? app->entryServersRef() : app->serversRef();
    const QHash<QString, Ping> &pings = app->pingsRef();
    const QStringList waitingList = app->pingWaiting();
    const QSet<QString> waiting(waitingList.begin(), waitingList.end());
    const QString q = m_query.trimmed();

    const QString exit = m_entry && app->entrySub() == app->currentSubId() ? s.profile.value_or(QString()) : QString();
    QList<Server> filtered;
    for (const Server &p : base)
        if (p.remarks != exit
            && (q.isEmpty() || p.name.contains(q, Qt::CaseInsensitive) || p.kind.contains(q, Qt::CaseInsensitive) || p.description.contains(q, Qt::CaseInsensitive)))
            filtered << p;

    QString selectedName;
    if (m_entry) {
        if (const auto chain = s.chainEntry(); chain && chain->first == app->entrySub())
            selectedName = chain->second;
    } else {
        selectedName = s.profile.value_or(QString());
    }
    const QStringList pinnedNames = (m_entry || !q.isEmpty()) ? QStringList() : s.pinned(app->currentSubId().isEmpty() ? std::nullopt : std::optional<QString>(app->currentSubId()));

    QList<Section> grouped = ::sections(filtered);
    if (s.view == u"single") {
        QList<Section> single;
        Section all;
        for (const Section &sec : std::as_const(grouped)) {
            if (sec.items.isEmpty())
                single << sec;
            all.items << sec.items;
        }
        single << all;
        grouped = single;
    }
    const QList<Section> list = sorted(grouped, s.sort, pings);
    const Server *selected = byRemarks(filtered, selectedName);
    if (selected && selected->placeholder)
        selected = nullptr;
    QList<const Server *> top;
    for (const QString &r : pinnedNames)
        for (const Server &p : std::as_const(filtered))
            if (p.remarks == r && !p.placeholder && (!selected || p.remarks != selected->remarks)) {
                top << &p;
                break;
            }
    QSet<QString> skip;
    for (const Server *p : std::as_const(top))
        skip.insert(p->remarks);
    if (selected && m_pinSelected)
        skip.insert(selected->remarks);

    const bool wideGrid = s.grid == u"1x1";
    if (wideGrid != m_wide) {
        m_wide = wideGrid;
        later();
    }
    const QString order = s.sort + u'/' + s.view + u'/' + s.grid;
    if (order != m_order) {
        if (!m_order.isEmpty() && m_made)
            m_dealAt = m_clock.elapsed();
        m_order = order;
    }
    QHash<QString, int> seen;
    int count = 0;
    QList<Row> out;
    out.reserve(base.size() + 16);
    auto tile = [&](const Server &p, bool wide, bool current) {
        const QString key = p.remarks + u'#' + QString::number(seen[p.remarks]++);
        const auto pit = pings.constFind(p.remarks);
        const bool hasPing = pit != pings.constEnd();
        const bool broken = hasPing && !pit->ms && !pit->error.isEmpty();
        QString line;
        bool lineError = false;
        if (broken) {
            line = pit->error.section(u':', 0, 0).section(QStringLiteral(" — "), 0, 0);
            lineError = true;
        } else if (!p.supported) {
            QStringList parts;
            if (!shortKind(p.kind).isEmpty())
                parts << shortKind(p.kind);
            parts << t("не поддерживается", "not supported");
            line = parts.join(QStringLiteral(" · "));
        } else if (!p.members.isEmpty() && s.serverInfo != u"none") {
            line = t("Балансер · %1", "Balancer · %1").arg(p.members.size());
        } else if (s.serverInfo == u"description") {
            line = !p.description.isEmpty() ? p.description : (wide ? p.kind : shortKind(p.kind));
        } else if (s.serverInfo == u"protocol") {
            line = wide ? p.kind : shortKind(p.kind);
        }
        ++count;
        Row r;
        r.key = key;
        r.kind = QStringLiteral("tile");
        r.remarks = p.remarks;
        r.name = p.name;
        r.flag = p.flag;
        r.line = line;
        r.lineError = lineError;
        r.supported = p.supported;
        r.selected = p.remarks == selectedName;
        r.focused = !m_focused.isEmpty() && p.remarks == m_focused;
        r.waiting = p.supported && waiting.contains(p.remarks);
        r.ping = hasPing && pit->ms ? *pit->ms : -1;
        r.pingFail = hasPing && !pit->ms;
        r.members = int(p.members.size());
        r.wide = wide;
        r.current = current;
        out << r;
    };
    auto header = [&](const QString &key, const QString &title, const QString &note, int total) {
        Row r;
        r.key = key;
        r.kind = QStringLiteral("header");
        r.title = title;
        r.note = note;
        r.count = total;
        out << r;
    };
    if (selected && m_pinSelected)
        tile(*selected, true, true);
    if (!top.isEmpty()) {
        header(QStringLiteral("h:pinned"), t("Закреплённые", "Pinned"), {}, int(top.size()));
        for (const Server *p : std::as_const(top))
            tile(*p, wideGrid, false);
    }
    QHash<QString, int> titles;
    for (const Section &full : list) {
        QList<const Server *> rest;
        for (const Server &p : full.items)
            if (!skip.contains(p.remarks))
                rest << &p;
        if (!full.items.isEmpty() && rest.isEmpty())
            continue;
        const QString key = QStringLiteral("%1#%2").arg(full.title).arg(titles[full.title]++);
        if (full.items.isEmpty()) {
            QStringList text;
            for (const QString &part : {full.title, full.note})
                if (!part.trimmed().isEmpty())
                    text << part;
            Row r;
            r.key = QStringLiteral("n:") + key;
            r.kind = QStringLiteral("notice");
            r.title = full.title;
            r.note = full.note;
            r.text = text.join(u'\n');
            out << r;
            continue;
        }
        if (!full.title.isEmpty() || !full.note.isEmpty())
            header(QStringLiteral("h:") + key, full.title, full.note, int(rest.size()));
        for (const Server *p : std::as_const(rest))
            tile(*p, wideGrid, false);
    }
    setRows(out);
    if (count != m_tiles) {
        m_tiles = count;
        emit tilesChanged();
    }
}

bool ServerGrid::wide() const { return m_wide; }

void ServerGrid::setViewWidth(qreal w)
{
    if (w == m_viewWidth)
        return;
    m_viewWidth = w;
    emit viewChanged();
    later();
}

void ServerGrid::setContentY(qreal y)
{
    if (y == m_contentY)
        return;
    m_contentY = y;
    emit viewChanged();
    const qreal top = std::floor(y / kViewStep) * kViewStep;
    if (top != m_viewTop) {
        m_viewTop = top;
        updateVisible();
    }
}

void ServerGrid::setViewHeight(qreal h)
{
    if (h == m_viewHeight)
        return;
    m_viewHeight = h;
    emit viewChanged();
    updateVisible();
}

void ServerGrid::setScope(const QString &s)
{
    if (s == m_scope)
        return;
    m_scope = s;
    emit scopeChanged();
    if (m_made)
        m_waveFresh = true;
}

void ServerGrid::enter(int dir)
{
    m_waveDir = dir;
    m_waveNow = true;
    later();
}

void ServerGrid::setNoticeHeight(const QString &key, qreal h)
{
    if (m_noticeHeights.value(key, -1) == h)
        return;
    m_noticeHeights.insert(key, h);
    later();
}

qreal ServerGrid::revealY(const QString &key, qreal maxY) const
{
    const int at = m_posOf.value(key, -1);
    if (at < 0)
        return -1;
    const GridCell::Place &p = m_pos.at(at);
    qreal to = m_contentY;
    if (p.y < m_contentY + 8)
        to = p.y - 8;
    else if (p.y + p.h > m_contentY + m_viewHeight - 8)
        to = p.y + p.h - m_viewHeight + 8;
    to = std::max<qreal>(0, std::min(maxY, to));
    return std::abs(to - m_contentY) > 0.5 ? to : -1;
}

void ServerGrid::addCell(GridCell *c)
{
    m_cells.insert(c->key(), c);
    const int at = m_posOf.value(c->key(), -1);
    const qreal top = m_viewTop - m_viewHeight - kViewStep, bottom = m_viewTop + m_viewHeight * 2 + kViewStep;
    if (at >= 0) {
        const GridCell::Place &p = m_pos.at(at);
        c->setNear(p.y + p.h > top && p.y < bottom);
        c->setLive(c->kind() != u"tile" || c->near() || m_retained.contains(c->key()));
        c->place(p, m_snapping, m_wave, m_waveDir);
    } else {
        c->setLive(c->kind() != u"tile");
    }
}

void ServerGrid::removeCell(GridCell *c)
{
    const auto it = m_cells.constFind(c->key());
    if (it != m_cells.constEnd() && it.value() == c)
        m_cells.erase(it);
}

void ServerGrid::later() { m_later.start(); }

void ServerGrid::soon()
{
    const qint64 wait = 280 - (m_clock.elapsed() - m_laidAt);
    if (!App::instance()->pinging() || wait <= 0)
        later();
    else if (!m_settle.isActive())
        m_settle.start(int(wait));
}

void ServerGrid::relayout()
{
    m_settle.stop();
    m_later.stop();
    const int n = int(m_rows.size());
    const qreal w = m_viewWidth - kInset * 2;
    if (w <= 0)
        return;
    const qreal minTile = m_wide ? 420 : 240;
    const qreal tileH = m_wide ? 60 : 52;
    const bool fresh = m_pos.size() != n || m_waveFresh;
    m_snapping = std::abs(w - m_laidWidth) > 0.5 || m_pos.isEmpty();
    m_laidWidth = w;
    const int cols = std::max(1, int(std::floor((w + kGap) / (minTile + kGap))));
    const qreal tw = (w - kGap * (cols - 1)) / cols;
    QList<GridCell::Place> out(n);
    QStringList keys(n);
    ++m_gen;
    qreal y = kInset;
    int col = 0;
    for (int i = 0; i < n; ++i) {
        const Row &d = m_rows.at(i);
        keys[i] = d.key;
        GridCell::Place &o = out[i];
        o.gen = m_gen;
        const bool tile = d.kind == u"tile";
        if (tile && !d.current) {
            o.x = kInset + col * (tw + kGap);
            o.y = y;
            o.w = tw;
            o.h = tileH;
            if (++col == cols) {
                col = 0;
                y += tileH + kGap;
            }
            continue;
        }
        if (col > 0) {
            col = 0;
            y += tileH + kGap;
        }
        o.x = kInset;
        o.w = w;
        if (tile) {
            o.y = y;
            o.h = 60;
            y += 60 + kGap + 4;
        } else if (d.kind == u"header") {
            o.y = y;
            o.h = d.note.isEmpty() ? 38 : 54;
            y += o.h + kGap;
        } else {
            o.y = y + 4;
            o.h = m_noticeHeights.value(keys[i], 0);
            y += o.h + 8 + kGap;
        }
    }
    if (col > 0)
        y += tileH + kGap;
    if (y != m_contentHeight) {
        m_contentHeight = y;
        emit contentHeightChanged();
    }

    auto moved = [](const GridCell *c, const GridCell::Place &o) {
        return std::abs(c->tx() - o.x) > 0.5 || std::abs(c->ty() - o.y) > 0.5;
    };
    if (n > 0) {
        const qint64 now = m_clock.elapsed();
        if (m_waveNow || (m_waveFresh && fresh)) {
            ++m_wave;
            m_waveAt = now;
            if (!m_waveNow)
                m_waveDir = 1;
            m_waveNow = false;
            m_waveFresh = false;
        }
        const bool waving = now - m_waveAt < 700;
        const bool dealing = !m_snapping && now - m_dealAt < 600;
        const qreal top = m_contentY, bottom = top + m_viewHeight;
        QList<int> seen;
        for (int i = 0; i < n; ++i)
            if (out[i].h > 0 && out[i].y + out[i].h > top && out[i].y < bottom)
                seen << i;
        const double step = std::min(20.0, 240.0 / std::max<qsizetype>(1, seen.size()));
        int added = 0;
        for (int k = 0; k < seen.size(); ++k) {
            GridCell::Place &o = out[seen[k]];
            const GridCell *c = m_cells.value(keys[seen[k]]);
            if (waving && (!c || c->wave() != m_wave)) {
                o.e = 1;
                o.d = std::max(0.0, 60 + k * step - double(now - m_waveAt));
            } else if (!c || !c->placed()) {
                o.e = 2;
                o.d = added++ * 16;
            } else if (dealing && moved(c, o)) {
                o.d = 1 + k * step * 0.6;
            }
        }
        for (int i = 0; i < n && dealing && m_dealAt > 0; ++i) {
            const GridCell *c = m_cells.value(keys[i]);
            if (c && c->placed() && moved(c, out[i]))
                m_dealAt = 0;
        }
    }
    QSet<QString> keep;
    for (int i = 0; i < n; ++i) {
        const GridCell *c = m_cells.value(keys[i]);
        if (c && c->placed() && c->near() && moved(c, out[i]))
            keep.insert(keys[i]);
    }
    m_retained = keep;
    if (!keep.isEmpty())
        m_release.start();
    m_pos = out;
    m_posKeys = keys;
    m_posOf.clear();
    for (int i = 0; i < n; ++i)
        m_posOf.insert(keys[i], i);
    for (GridCell *c : std::as_const(m_cells)) {
        const int at = m_posOf.value(c->key(), -1);
        if (at >= 0)
            c->place(m_pos.at(at), m_snapping, m_wave, m_waveDir);
    }
    updateVisible();
    m_laidAt = m_clock.elapsed();
    m_made = true;
    m_unsnap.start();
}

void ServerGrid::updateVisible()
{
    const qreal top = m_viewTop - m_viewHeight - kViewStep, bottom = m_viewTop + m_viewHeight * 2 + kViewStep;
    auto near = [&](const GridCell::Place &p) { return p.y + p.h > top && p.y < bottom; };
    QStringList keys;
    for (int i = 0; i < m_pos.size(); ++i) {
        const QString &key = m_posKeys.at(i);
        const int row = m_rowOf.value(key, -1);
        if (row >= 0 && (m_rows.at(row).kind != u"tile" || m_retained.contains(key) || near(m_pos.at(i))))
            keys << key;
    }
    for (GridCell *c : std::as_const(m_cells)) {
        const int at = m_posOf.value(c->key(), -1);
        const bool on = at >= 0 && near(m_pos.at(at));
        c->setNear(on);
        c->setLive(c->kind() != u"tile" || on || m_retained.contains(c->key()));
    }
    m_shown = keys;
    showRows(keys);
}

void ServerGrid::setRows(const QList<Row> &rows)
{
    bool inOut = rows.size() != m_rows.size(), moved = false, reshaped = false;
    QHash<QString, int> rowOf;
    rowOf.reserve(rows.size());
    for (int i = 0; i < rows.size(); ++i)
        rowOf.insert(rows.at(i).key, i);
    for (int i = 0; i < m_rows.size() && !inOut; ++i) {
        const Row &was = m_rows.at(i);
        const int at = rowOf.value(was.key, -1);
        if (at < 0) {
            inOut = true;
            break;
        }
        const Row &now = rows.at(at);
        moved = moved || at != i;
        reshaped = reshaped || now.kind != was.kind || now.current != was.current || now.note != was.note;
    }
    m_rows = rows;
    m_rowOf = rowOf;
    if (inOut)
        later();
    else if (moved || reshaped)
        soon();
    showRows(m_shown);
}

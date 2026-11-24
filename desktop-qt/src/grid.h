#pragma once

#include "gridcell.h"

#include <QAbstractListModel>
#include <QElapsedTimer>
#include <QHash>
#include <QPointer>
#include <QQmlEngine>
#include <QTimer>
#include <QSet>
#include <QVariantMap>

class KeyedModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Provided by C++")
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    explicit KeyedModel(const QStringList &roles, QObject *parent = nullptr);

    void setItems(const QList<QVariantMap> &items);
    int count() const { return int(m_items.size()); }
    const QList<QVariantMap> &items() const { return m_items; }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    Q_INVOKABLE QVariantMap get(int row) const;
    Q_INVOKABLE int indexOf(const QString &key) const;
    Q_INVOKABLE int roleIndex(const QString &name) const;

signals:
    void countChanged();

private:
    QStringList m_roles;
    QList<QVariantMap> m_items;
};

class ServerGrid : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(KeyedModel *visibleItems READ visibleItems CONSTANT)
    Q_PROPERTY(qreal viewWidth READ viewWidth WRITE setViewWidth NOTIFY viewChanged)
    Q_PROPERTY(qreal contentY READ contentY WRITE setContentY NOTIFY viewChanged)
    Q_PROPERTY(qreal viewHeight READ viewHeight WRITE setViewHeight NOTIFY viewChanged)
    Q_PROPERTY(qreal contentHeight READ contentHeight NOTIFY contentHeightChanged)
    Q_PROPERTY(qreal inset READ inset CONSTANT)
    Q_PROPERTY(QString scope READ scope WRITE setScope NOTIFY scopeChanged)
    Q_PROPERTY(QString query READ query WRITE setQuery NOTIFY queryChanged)
    Q_PROPERTY(bool entry READ entry WRITE setEntry NOTIFY entryChanged)
    Q_PROPERTY(bool pinSelected READ pinSelected WRITE setPinSelected NOTIFY pinSelectedChanged)
    Q_PROPERTY(QString focused READ focused WRITE setFocused NOTIFY focusedChanged)
    Q_PROPERTY(int tiles READ tiles NOTIFY tilesChanged)

public:
    explicit ServerGrid(QObject *parent = nullptr);

    KeyedModel *visibleItems() const { return m_visibleItems; }
    qreal viewWidth() const { return m_viewWidth; }
    void setViewWidth(qreal w);
    qreal contentY() const { return m_contentY; }
    void setContentY(qreal y);
    qreal viewHeight() const { return m_viewHeight; }
    void setViewHeight(qreal h);
    qreal contentHeight() const { return m_contentHeight; }
    qreal inset() const { return kInset; }
    QString scope() const { return m_scope; }
    void setScope(const QString &s);
    QString query() const { return m_query; }
    void setQuery(const QString &q);
    bool entry() const { return m_entry; }
    void setEntry(bool on);
    bool pinSelected() const { return m_pinSelected; }
    void setPinSelected(bool on);
    QString focused() const { return m_focused; }
    void setFocused(const QString &f);
    int tiles() const { return m_tiles; }
    Q_INVOKABLE QStringList pingTargets();
    Q_INVOKABLE void enter(int dir);
    Q_INVOKABLE void setNoticeHeight(const QString &key, qreal h);
    Q_INVOKABLE qreal revealY(const QString &key, qreal maxY) const;

    void addCell(GridCell *c);
    void removeCell(GridCell *c);

signals:
    void viewChanged();
    void contentHeightChanged();
    void scopeChanged();
    void queryChanged();
    void entryChanged();
    void pinSelectedChanged();
    void focusedChanged();
    void tilesChanged();

private:
    static constexpr qreal kInset = 6;
    static constexpr qreal kGap = 8;
    static constexpr qreal kViewStep = 200;

    void schedule();
    void rebuild();
    struct Row {
        QString key, kind, title, note, text, remarks, name, flag, line;
        int count = 0, ping = -1, members = 0;
        bool lineError = false, supported = false, selected = false, focused = false, waiting = false, pingFail = false,
             wide = false, current = false;
        QVariantMap map() const;
    };
    void setRows(const QList<Row> &rows);
    void showRows(const QStringList &keys);
    void later();
    void soon();
    void relayout();
    void updateVisible();
    bool wide() const;

    QList<Row> m_rows;
    QHash<QString, int> m_rowOf;
    QStringList m_shown;
    KeyedModel *m_visibleItems;
    QString m_query;
    bool m_entry = false;
    bool m_pinSelected = true;
    QString m_focused;
    int m_tiles = 0;
    QTimer m_debounce;

    qreal m_viewWidth = 0, m_contentY = 0, m_viewHeight = 0, m_contentHeight = 0;
    qreal m_viewTop = 0, m_laidWidth = 0;
    QString m_scope, m_order;
    bool m_wide = false;
    QList<GridCell::Place> m_pos;
    QStringList m_posKeys;
    QHash<QString, int> m_posOf;
    QHash<QString, GridCell *> m_cells;
    QHash<QString, qreal> m_noticeHeights;
    QSet<QString> m_retained;
    int m_gen = 0;
    bool m_made = false;
    bool m_snapping = true;
    int m_wave = 0;
    qint64 m_waveAt = -100000;
    int m_waveDir = 1;
    bool m_waveNow = false;
    bool m_waveFresh = false;
    qint64 m_dealAt = 0;
    qint64 m_laidAt = -100000;
    QElapsedTimer m_clock;
    QTimer m_later, m_settle, m_release, m_unsnap;
};

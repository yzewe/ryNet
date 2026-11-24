#pragma once

#include <QAbstractListModel>
#include <QQmlEngine>
#include <QTimer>

class LogModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString level READ level WRITE setLevel NOTIFY filterChanged)
    Q_PROPERTY(QString query READ query WRITE setQuery NOTIFY filterChanged)
    Q_PROPERTY(bool paused READ paused WRITE setPaused NOTIFY pausedChanged)
    Q_PROPERTY(bool active READ active WRITE setActive NOTIFY activeChanged)
    Q_PROPERTY(int total READ total NOTIFY countsChanged)
    Q_PROPERTY(int warnings READ warnings NOTIFY countsChanged)
    Q_PROPERTY(int errors READ errors NOTIFY countsChanged)
    Q_PROPERTY(int count READ count NOTIFY countsChanged)

public:
    struct Line {
        QString time, level, tag, text, raw;
        bool fresh = false;
    };

    explicit LogModel(QObject *parent = nullptr);

    QString level() const { return m_level; }
    void setLevel(const QString &l);
    QString query() const { return m_query; }
    void setQuery(const QString &q);
    bool paused() const { return m_paused; }
    void setPaused(bool on);
    bool active() const { return m_active; }
    void setActive(bool on);
    int total() const { return int(m_parsed.size()); }
    int warnings() const { return m_warnings; }
    int errors() const { return m_errors; }
    int count() const { return int(m_shown.size()); }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    Q_INVOKABLE QString text() const;
    Q_INVOKABLE void clear();
    Q_INVOKABLE void reload();

    static Line parse(const QString &l);

signals:
    void filterChanged();
    void pausedChanged();
    void activeChanged();
    void countsChanged();

private:
    void poll();
    bool matches(const Line &l) const;
    void refilter();

    QString m_level = QStringLiteral("all");
    QString m_query;
    bool m_paused = false;
    bool m_active = false;
    QStringList m_raw;
    QList<Line> m_parsed;
    QList<int> m_shown;
    int m_warnings = 0, m_errors = 0;
    QTimer m_timer;
};

#pragma once

#include <QColor>
#include <QElapsedTimer>
#include <QPointF>
#include <QPointer>
#include <QQmlEngine>
#include <QQuickItem>
#include <QVariantList>

class Ui : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QQuickItem *overlay READ overlay WRITE setOverlay NOTIFY overlayChanged)
    Q_PROPERTY(int widthClass READ widthClass WRITE setWidthClass NOTIFY widthClassChanged)
    Q_PROPERTY(bool compact READ compact NOTIFY widthClassChanged)
    Q_PROPERTY(bool split READ split WRITE setSplit NOTIFY splitChanged)
    Q_PROPERTY(qreal snackHeight READ snackHeight WRITE setSnackHeight NOTIFY snackHeightChanged)
    Q_PROPERTY(qreal outer READ outer WRITE setOuter NOTIFY sectionChanged)
    Q_PROPERTY(qreal inner READ inner WRITE setInner NOTIFY sectionChanged)
    Q_PROPERTY(qreal gap READ gap WRITE setGap NOTIFY sectionChanged)
    Q_PROPERTY(QQuickItem *tipItem READ tipItem WRITE setTipItem NOTIFY tipItemChanged)
    Q_PROPERTY(QString tipText READ tipText WRITE setTipText NOTIFY tipTextChanged)
    Q_PROPERTY(QString fontFamily READ fontFamily NOTIFY fontFamilyChanged)
    Q_PROPERTY(QVariantList dnsPresets READ dnsPresets CONSTANT)
    Q_PROPERTY(QVariantList serverDnsPresets READ serverDnsPresets CONSTANT)

public:
    static Ui *instance();
    static Ui *create(QQmlEngine *, QJSEngine *);

    QQuickItem *overlay() const { return m_overlay; }
    void setOverlay(QQuickItem *o);
    int widthClass() const { return m_widthClass; }
    void setWidthClass(int c);
    bool compact() const { return m_widthClass == 0; }
    bool split() const { return m_split; }
    void setSplit(bool on);
    qreal snackHeight() const { return m_snackHeight; }
    void setSnackHeight(qreal h);
    qreal outer() const { return m_outer; }
    void setOuter(qreal v);
    qreal inner() const { return m_inner; }
    void setInner(qreal v);
    qreal gap() const { return m_gap; }
    QString fontFamily() const { return m_fontFamily; }
    void setFontFamily(const QString &f);
    void setGap(qreal v);
    QQuickItem *tipItem() const { return m_tipItem; }
    void setTipItem(QQuickItem *i);
    QString tipText() const { return m_tipText; }
    void setTipText(const QString &t);
    QVariantList dnsPresets() const;
    QVariantList serverDnsPresets() const;

    Q_INVOKABLE QString tr(const QString &ru, const QString &en) const;
    Q_INVOKABLE QString geoPage(const QString &kind, const QString &name) const;
    Q_INVOKABLE void showTip(QQuickItem *item, const QString &text);
    Q_INVOKABLE void hideTip(QQuickItem *item);
    Q_INVOKABLE QString flagCode(const QString &emoji) const;
    Q_INVOKABLE QStringList splitFlag(const QString &name) const;
    Q_INVOKABLE QString dnsLabel(const QString &value) const;
    Q_INVOKABLE QString dnsChoice(const QString &value) const;
    Q_INVOKABLE bool serverDnsPreset(const QString &value) const;
    Q_INVOKABLE QStringList dnsOptions() const;
    Q_INVOKABLE QStringList dnsLabels() const;
    Q_INVOKABLE QString dnsSummary(const QString &value) const;
    Q_INVOKABLE QColor pingColor(int ms) const;
    Q_INVOKABLE QVariantList setPreview(const QVariantMap &set) const;
    Q_INVOKABLE int nearest(const QVariantList &steps, double value) const;
    Q_INVOKABLE void groupEdges(QQuickItem *column, bool first, bool last, bool folds) const;
    Q_INVOKABLE QVariantMap errorReason(const QString &message) const;
    Q_INVOKABLE QString shortLink(const QString &link) const;
    Q_INVOKABLE QString joined(const QStringList &parts, const QString &sep) const;
    Q_INVOKABLE QStringList suggest(const QStringList &list, const QString &text, int limit) const;
    Q_INVOKABLE QVariantList usable(const QVariantList &servers) const;
    Q_INVOKABLE int indexOfKey(const QVariantList &list, const QString &field, const QVariant &value) const;
    Q_INVOKABLE QStringList pluck(const QVariantList &list, const QString &field) const;
    Q_INVOKABLE QStringList toggled(const QStringList &list, const QString &item) const;
    Q_INVOKABLE int countEntries(const QVariantMap &lists) const;
    Q_INVOKABLE QString hostList(const QString &text) const;
    Q_INVOKABLE QStringList without(const QStringList &list, const QString &value) const;
    Q_INVOKABLE QStringList merged(const QStringList &list, const QStringList &more) const;

    Q_INVOKABLE bool within(QQuickItem *item, QQuickItem *root) const;
    Q_INVOKABLE void moveFocus(QQuickItem *root, QQuickItem *from, int by) const;
    Q_INVOKABLE QQuickItem *takeFocus(QQuickItem *scope, QQuickItem *returnTo) const;
    Q_INVOKABLE void giveFocusBack(QQuickItem *layer, QQuickItem *returnTo) const;
    Q_INVOKABLE qreal menuItems(QQuickItem *column, QObject *menu) const;
    Q_INVOKABLE void menuSweep(QQuickItem *column, qreal edge, bool upward) const;
    Q_INVOKABLE QPointF growFrom(qreal w, qreal h) const;

    Q_INVOKABLE void cascadeIn(QQuickItem *root, int dir);
    Q_INVOKABLE void cascade(QQuickItem *item, qreal dir, int delay);
    Q_INVOKABLE void cascadeAt(QQuickItem *item, qreal dir, qreal y, double since);
    Q_INVOKABLE void cascadeDown(QQuickItem *root, qreal dir, double since);

signals:
    void overlayChanged();
    void widthClassChanged();
    void snackHeightChanged();
    void sectionChanged();
    void fontFamilyChanged();
    void splitChanged();
    void tipItemChanged();
    void tipTextChanged();

protected:
    bool eventFilter(QObject *o, QEvent *e) override;

private:
    explicit Ui(QObject *parent = nullptr);

    QPointer<QQuickItem> m_overlay;
    int m_widthClass = 2;
    qreal m_snackHeight = 0;
    qreal m_outer = 16, m_inner = 4, m_gap = 2;
    QString m_fontFamily;
    bool m_split = false;
    QPointer<QQuickItem> m_tipItem;
    QString m_tipText;
    QPointF m_pressAt;
    QElapsedTimer m_pressed;
};

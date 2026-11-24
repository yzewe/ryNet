#include "app.h"
#include "ui.h"

#include <QRegularExpression>

namespace {
QStringList words(const QString &text, const QRegularExpression &by) { return text.split(by, Qt::SkipEmptyParts); }

const QRegularExpression &spaces()
{
    static const QRegularExpression re(QStringLiteral("\\s+"));
    return re;
}
const QRegularExpression &dnsSeparators()
{
    static const QRegularExpression re(QStringLiteral("[\\s,]+"));
    return re;
}
const QRegularExpression &hostSeparators()
{
    static const QRegularExpression re(QStringLiteral("[\\s,;]+"));
    return re;
}

bool number(const QString &t, int lo, int hi)
{
    bool ok = false;
    const int v = t.toInt(&ok);
    return ok && !t.startsWith(u'+') && !t.startsWith(u'-') && v >= lo && v <= hi;
}
}

static const QString kServerName = QStringLiteral("serverName:");

QString App::fieldText(const QString &field) const
{
    const Settings &s = m_settings;
    if (field.startsWith(kServerName))
        return field.mid(kServerName.size());
    if (field == u"dns")
        return Ui::instance()->dnsChoice(s.dnsServers) == u"custom" ? s.dnsServers : QString();
    if (field == u"mtu")
        return s.mtu == 0 ? QString() : QString::number(s.mtu);
    if (field == u"port")
        return QString::number(s.port);
    if (field == u"fragment")
        return s.fragmentLength + u' ' + s.fragmentInterval;
    if (field == u"pingUrl")
        return s.pingUrl;
    if (field == u"ipUrl")
        return s.ipUrl;
    if (field == u"serverDnsCustom")
        return words(s.serverDnsCustom, dnsSeparators()).join(u'\n');
    if (field == u"subHeaders")
        return s.subHeaders;
    if (field == u"hwid")
        return s.hwid;
    if (field == u"autoHome" || field == u"autoFree")
        return words(field == u"autoHome" ? s.autoHome : s.autoFree, hostSeparators()).join(u'\n');
    if (field == u"userRules")
        return s.userRules;
    return {};
}

bool App::fieldValid(const QString &field, const QString &text) const
{
    if (field.startsWith(kServerName))
        return !text.trimmed().isEmpty() && text.size() <= 80;
    if (field == u"dns") {
        const QStringList all = words(text, dnsSeparators());
        return std::all_of(all.begin(), all.end(), [this](const QString &x) { return validDns(x); });
    }
    if (field == u"mtu")
        return number(text, 1280, 9000);
    if (field == u"port")
        return number(text, 1024, 65535);
    if (field == u"fragment") {
        const QStringList p = words(text, spaces());
        return p.size() == 2 && validRange(p[0], 65535) && validRange(p[1], 1000);
    }
    if (field == u"serverDnsCustom") {
        static const QRegularExpression hop(QStringLiteral("^([0-9a-fA-F.:\\[\\]]+|(udp|tls|https)://[^\\s/]+(/\\S*)?)$"), QRegularExpression::CaseInsensitiveOption);
        const QStringList all = words(text, dnsSeparators());
        return !all.isEmpty() && std::all_of(all.begin(), all.end(), [](const QString &x) { return hop.match(x).hasMatch(); });
    }
    if (field == u"pingUrl" || field == u"ipUrl") {
        static const QRegularExpression url(QStringLiteral("^https?://[^\\s/]+(/\\S*)?$"));
        return url.match(text).hasMatch();
    }
    if (field == u"hwid")
        return text.size() >= 4 && text.size() <= 64 && !text.contains(spaces());
    if (field == u"autoHome" || field == u"autoFree")
        return !autoHosts(text).isEmpty();
    return true;
}

void App::saveField(const QString &field, const QString &text)
{
    if (field.startsWith(kServerName)) {
        renameLocal(field.mid(kServerName.size()), text);
    } else if (field == u"dns") {
        set(QStringLiteral("dnsServers"), text, true);
    } else if (field == u"mtu") {
        set(QStringLiteral("mtu"), text.isEmpty() ? 0 : text.toInt(), true);
    } else if (field == u"port") {
        set(QStringLiteral("port"), text.toInt(), true);
    } else if (field == u"fragment") {
        const QStringList p = words(text, spaces());
        if (p.size() == 2) {
            set(QStringLiteral("fragmentLength"), p[0]);
            set(QStringLiteral("fragmentInterval"), p[1], true);
        }
    } else if (field == u"autoHome" || field == u"autoFree") {
        set(field, autoHosts(text));
    } else if (field == u"userRules") {
        set(field, text, true);
        updateGeo();
    } else if (field == u"serverDnsCustom") {
        Settings next = m_settings;
        next.serverDns = QStringLiteral("custom");
        next.serverDnsCustom = words(text, dnsSeparators()).join(u' ');
        apply(next, true);
    } else if (field == u"pingUrl" || field == u"ipUrl" || field == u"subHeaders" || field == u"hwid") {
        set(field, text);
    }
}

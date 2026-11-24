#include "grid.h"
#include "app.h"
#include "core.h"
#include "store.h"

#include <QAbstractItemModelTester>
#include <QPersistentModelIndex>
#include <QSignalSpy>
#include <QTest>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <algorithm>

QList<QVariantMap> rows(const QString &prefix = QStringLiteral("server")) {
    QList<QVariantMap> out;
    for (int i = 0; i < 3000; ++i)
        out << QVariantMap{{"key", prefix + QString::number(i)}, {"name", QStringLiteral("Germany %1").arg(i)}, {"waiting", false}};
    return out;
}

class GridModelTests : public QObject {
    Q_OBJECT
private slots:
    void pingSomeAndRefreshPreserveSelectionAndOtherRuns();
    void signalsAndIdentity() {
        KeyedModel m({"key", "name", "waiting"});
        QAbstractItemModelTester tester(&m, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy reset(&m, &QAbstractItemModel::modelReset);
        QSignalSpy inserted(&m, &QAbstractItemModel::rowsInserted);
        QSignalSpy moved(&m, &QAbstractItemModel::rowsMoved);
        QSignalSpy changed(&m, &QAbstractItemModel::dataChanged);
        QSignalSpy removed(&m, &QAbstractItemModel::rowsRemoved);
        QSignalSpy count(&m, &KeyedModel::countChanged);
        auto initial = rows(); initial = initial.mid(0, 4);
        m.setItems(initial);
        QCOMPARE(m.rowCount(), 4);
        int insertedCount = 0;
        for (const auto &s : inserted) insertedCount += s[2].toInt() - s[1].toInt() + 1;
        QCOMPARE(insertedCount, 4); QCOMPARE(count.size(), 1);
        QPersistentModelIndex survivor(m.index(2));
        auto next = initial; std::reverse(next.begin(), next.end());
        next[1]["name"] = QStringLiteral("changed");
        m.setItems(next);
        QCOMPARE(survivor.row(), 1);
        QCOMPARE(survivor.data(Qt::UserRole).toString(), QStringLiteral("server2"));
        QVERIFY(!moved.isEmpty()); QCOMPARE(changed.size(), 1);
        QCOMPARE(qvariant_cast<QList<int>>(changed[0][2]), QList<int>{Qt::UserRole + 1});
        QCOMPARE(count.size(), 1);
        next.removeAt(3); next.insert(1, QVariantMap{{"key", "new1"}, {"name", "New 1"}});
        next.insert(2, QVariantMap{{"key", "new2"}, {"name", "New 2"}});
        m.setItems(next);
        QCOMPARE(m.items(), next); QCOMPARE(survivor.row(), 3);
        QCOMPARE(removed.size(), 1); QCOMPARE(count.size(), 2);
        m.setItems(next); QCOMPARE(changed.size(), 1);
        m.setItems({}); QCOMPARE(m.rowCount(), 0); QVERIFY(!survivor.isValid());
        QCOMPARE(reset.size(), 0);
    }
    void mixedUpdates() {
        KeyedModel m({"key", "name", "waiting"});
        QAbstractItemModelTester tester(&m, QAbstractItemModelTester::FailureReportingMode::QtTest);
        auto current = rows().mid(0, 24);
        m.setItems(current);
        QList<QPersistentModelIndex> indexes;
        for (int i = 0; i < current.size(); ++i) indexes << QPersistentModelIndex(m.index(i));
        auto updated = current;
        std::reverse(updated.begin(), updated.end());
        updated.insert(0, QVariantMap{{"key", "newA"}});
        updated.insert(1, QVariantMap{{"key", "newB"}});
        updated.insert(9, QVariantMap{{"key", "newC"}});
        updated.insert(10, QVariantMap{{"key", "newD"}});
        updated << QVariantMap{{"key", "newE"}} << QVariantMap{{"key", "newF"}};
        m.setItems(updated);
        QCOMPARE(m.items(), updated);
        for (int i = 0; i < indexes.size(); ++i) {
            QVERIFY(indexes[i].isValid());
            QCOMPARE(indexes[i].data(Qt::UserRole), current[i].value("key"));
        }
    }
};
void GridModelTests::pingSomeAndRefreshPreserveSelectionAndOtherRuns()
{
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QByteArray oldConfig = qgetenv("XDG_CONFIG_HOME");
    const QByteArray oldCore = qgetenv("RYNET_CORE");
    const QByteArray oldLog = qgetenv("RYNET_TEST_COMMANDS");
    App *app = nullptr;
    const auto cleanup = qScopeGuard([&] {
        if (app) app->shutdown();
        for (const auto &value : {qMakePair(QByteArray("XDG_CONFIG_HOME"), oldConfig),
                 qMakePair(QByteArray("RYNET_CORE"), oldCore), qMakePair(QByteArray("RYNET_TEST_COMMANDS"), oldLog)}) {
            if (value.second.isNull()) qunsetenv(value.first.constData());
            else qputenv(value.first.constData(), value.second);
        }
    });
    const QString log = dir.filePath(QStringLiteral("commands.jsonl"));
    const QString executable = dir.filePath(QStringLiteral("fake-core.py"));
    QFile script(executable);
    QVERIFY(script.open(QIODevice::WriteOnly));
    script.write(R"PY(#!/usr/bin/env python3
import json, os, sys
if '--tray' in sys.argv:
    for line in sys.stdin:
        if json.loads(line).get('cmd') == 'quit':
            break
    sys.exit(0)
print(json.dumps({'ev': 'ready', 'version': 'test', 'tunCapable': False}), flush=True)
fetches = 0
for line in sys.stdin:
    q = json.loads(line)
    with open(os.environ['RYNET_TEST_COMMANDS'], 'a') as f:
        f.write(json.dumps(q) + '\n')
    cmd = q.get('cmd')
    if cmd == 'quit':
        break
    if 'req' not in q:
        continue
    r = {'req': q['req']}
    if cmd == 'describe':
        name = 'a1' if q.get('text', '').startswith('A') else 'b1'
        r['profiles'] = [{'remarks': name, 'supported': True, 'kind': 'VLESS'}]
    elif cmd == 'fetch':
        fetches += 1
        sid = q['sub']
        r['sub'] = {'id': sid, 'url': q['url'], 'title': sid, 'updatedAt': 1780000000000}
        r['body'] = sid + '-refreshed-' + str(fetches)
    elif cmd == 'caps':
        r['hwid'] = 'test'
    print(json.dumps(r), flush=True)
)PY");
    script.close();
    QVERIFY(script.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));
    qputenv("XDG_CONFIG_HOME", dir.path().toUtf8());
    qputenv("RYNET_CORE", executable.toUtf8());
    qputenv("RYNET_TEST_COMMANDS", log.toUtf8());
    Sub a; a.id = QStringLiteral("A"); a.url = QStringLiteral("https://sub.example/a"); a.title = QStringLiteral("A");
    Sub b; b.id = QStringLiteral("B"); b.url = QStringLiteral("https://sub.example/b"); b.title = QStringLiteral("B");
    Settings settings;
    settings.sub = a.id;
    settings.profile = QStringLiteral("a1");
    settings.autoSelect = true;
    settings.autoUpdate = false;
    settings.pingOnStart = false;
    settings.exitFlags = false;
    Store::save({a, b}, settings);
    Store::saveBody(a.id, QStringLiteral("A"));
    Store::saveBody(b.id, QStringLiteral("B"));
    app = App::instance();
    app->start(false);
    QTRY_COMPARE(app->supportedCount(), 1);
    Core *core = app->findChild<Core *>();
    QVERIFY(core);
    const auto commands = [&] {
        QList<QJsonObject> out;
        QFile f(log);
        if (f.open(QIODevice::ReadOnly)) {
            for (const auto &line : f.readAll().split('\n')) {
                const auto o = QJsonDocument::fromJson(line).object();
                if (o.value("cmd").toString() == u"ping") out << o;
            }
        }
        return out;
    };
    const auto unchanged = [&] {
        QCOMPARE(app->currentSubId(), QStringLiteral("A"));
        QCOMPARE(app->settingsVariant().value("profile").toString(), QStringLiteral("a1"));
        QVERIFY(app->settingsVariant().value("autoSelect").toBool());
        QCOMPARE(Store::settings().sub.value_or(QString()), QStringLiteral("A"));
        QCOMPARE(Store::settings().profile.value_or(QString()), QStringLiteral("a1"));
        QVERIFY(Store::settings().autoSelect);
    };
    const auto result = [&](double generation, const QString &name, int ms) {
        emit core->event(QJsonObject{{"ev", "ping"}, {"generation", generation}, {"name", name}, {"ms", ms}});
    };
    const auto done = [&](double generation) {
        emit core->event(QJsonObject{{"ev", "pingDone"}, {"generation", generation}});
    };
    unchanged();
    app->pingSome({QStringLiteral("b1")}, QStringLiteral("B"));
    QTRY_COMPARE(commands().size(), 1);
    const double first = commands().last().value("generation").toDouble();
    QVERIFY(first > 0);
    unchanged();
    result(first, QStringLiteral("b1"), 77);
    QCOMPARE(app->pingIn(QStringLiteral("B"), QStringLiteral("b1")).value("ms").toInt(), 77);
    done(first);
    QVERIFY(!app->pinging());

    app->pingSome({QStringLiteral("a1")}, QStringLiteral("A"));
    QTRY_COMPARE(commands().size(), 2);
    const double oldA = commands().last().value("generation").toDouble();
    result(oldA, QStringLiteral("a1"), 42);
    QVERIFY(!app->pingIn(QStringLiteral("A"), QStringLiteral("a1")).isEmpty());
    app->refresh(QStringLiteral("A"));
    QTRY_VERIFY(!app->busy());
    QCOMPARE(Store::body(QStringLiteral("A")).value_or(QString()), QStringLiteral("A-refreshed-1"));
    QVERIFY(!app->pinging());
    QVERIFY(app->pingIn(QStringLiteral("A"), QStringLiteral("a1")).isEmpty());
    result(oldA, QStringLiteral("a1"), 1);
    done(oldA);
    QVERIFY(app->pingIn(QStringLiteral("A"), QStringLiteral("a1")).isEmpty());
    QCOMPARE(app->pingIn(QStringLiteral("B"), QStringLiteral("b1")).value("ms").toInt(), 77);
    unchanged();

    app->pingSome({QStringLiteral("b1"), QStringLiteral("b2")}, QStringLiteral("B"));
    QTRY_COMPARE(commands().size(), 3);
    const double currentB = commands().last().value("generation").toDouble();
    QVERIFY(currentB != oldA);
    result(currentB, QStringLiteral("b1"), 66);
    QVERIFY(app->pingWaitingFor(QStringLiteral("b2"), QStringLiteral("B")));
    QVERIFY(!app->pingWaitingFor(QStringLiteral("b2")));
    app->refresh(QStringLiteral("A"));
    QTRY_VERIFY(!app->busy());
    QCOMPARE(Store::body(QStringLiteral("A")).value_or(QString()), QStringLiteral("A-refreshed-2"));
    QVERIFY(app->pinging());
    QCOMPARE(app->pingIn(QStringLiteral("B"), QStringLiteral("b1")).value("ms").toInt(), 66);
    QVERIFY(app->pingWaitingFor(QStringLiteral("b2"), QStringLiteral("B")));
    result(oldA, QStringLiteral("b2"), 2);
    done(oldA);
    QVERIFY(app->pinging());
    QVERIFY(app->pingIn(QStringLiteral("B"), QStringLiteral("b2")).isEmpty());
    result(currentB, QStringLiteral("b2"), 88);
    done(currentB);
    QVERIFY(!app->pinging());
    QCOMPARE(app->pingIn(QStringLiteral("B"), QStringLiteral("b1")).value("ms").toInt(), 66);
    QCOMPARE(app->pingIn(QStringLiteral("B"), QStringLiteral("b2")).value("ms").toInt(), 88);
    unchanged();
}

QTEST_MAIN(GridModelTests)
#include "gridtests.moc"
